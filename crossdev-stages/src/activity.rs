//! em's activity events, read from a FIFO in the sandbox log dir and drawn on the terminal
//!
//! hakoniwa closes every inherited fd above stderr, so em writes `--activity-jsonl` to a FIFO in
//! the log dir, which the sandbox sees at `/var/log`. Unknown events and fields are skipped.

use std::fs::File;
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::OnceLock;
use std::thread::JoinHandle;
use std::time::Duration;

use camino::{Utf8Path, Utf8PathBuf};
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use serde::Deserialize;

/// The FIFO's name in the log dir
pub const FIFO: &str = "em-activity.fifo";

/// How long the reader waits for an event before refreshing the bars
const TICK: Duration = Duration::from_millis(250);

/// Build log lines shown for a failed package
const FAILURE_LINES: usize = 20;

/// How em's work is shown
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum Output {
    /// em's own output, as it prints it
    #[default]
    Plain,
    /// A line per step, and the failures; em's output goes to a log
    Quiet,
    /// Progress bars for the steps and the packages; em's output goes to a log
    Fancy,
}

static OUTPUT: OnceLock<Output> = OnceLock::new();

/// Set the output mode once, at startup
pub fn set_output(output: Output) {
    let _ = OUTPUT.set(output);
}

pub fn output() -> Output {
    OUTPUT.get().copied().unwrap_or_default()
}

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Event {
    StepStart {
        index: u32,
        of: u32,
        label: String,
    },
    StepSkipped {
        index: u32,
        of: u32,
    },
    StepEnd {
        index: u32,
        of: u32,
        label: String,
        ok: bool,
        seconds: f64,
    },
    SessionStart {
        job_id: String,
        plan_total: u32,
    },
    SessionEnd {
        job_id: String,
    },
    PkgStart {
        job_id: String,
        cpv: String,
        #[serde(default)]
        log: Option<String>,
    },
    PhaseEnter {
        job_id: String,
        cpv: String,
        phase: String,
    },
    PkgEnd {
        job_id: String,
        cpv: String,
        ok: bool,
        seconds: f64,
        #[serde(default)]
        failed_phase: Option<String>,
        #[serde(default)]
        log: Option<String>,
    },
    PlanFailed {
        message: String,
    },
    #[serde(other)]
    Other,
}

/// Where the sandbox's paths are on the host
#[derive(Clone)]
struct Paths {
    sandbox_dir: Utf8PathBuf,
    log_dir: Utf8PathBuf,
}

impl Paths {
    fn host(&self, path: &str) -> Utf8PathBuf {
        match path.strip_prefix("/var/log/") {
            Some(rest) => self.log_dir.join(rest),
            None => self.sandbox_dir.join(path.trim_start_matches('/')),
        }
    }

    /// A failed package: its phase, its build log's host path and last lines
    fn failure(&self, cpv: &str, phase: Option<&str>, log: Option<&str>) -> String {
        let mut out = match phase {
            Some(phase) => format!("{cpv} failed in {phase}"),
            None => format!("{cpv} failed"),
        };
        if let Some(log) = log.map(|log| self.host(log)) {
            for line in tail(&log, FAILURE_LINES) {
                out.push_str("\n  | ");
                out.push_str(&line);
            }
            out.push_str(&format!("\n  build log: {log}"));
        }
        out
    }
}

/// Consecutive skipped steps, reported as one line
#[derive(Default)]
struct Skipped(Option<(u32, u32, u32)>);

impl Skipped {
    fn add(&mut self, index: u32, of: u32) {
        let first = self.0.map_or(index, |(first, _, _)| first);
        self.0 = Some((first, index, of));
    }

    fn take(&mut self) -> Option<String> {
        let (first, last, of) = self.0.take()?;
        let range = if first == last {
            format!("{first}")
        } else {
            format!("{first}-{last}")
        };
        Some(format!("[{range}/{of}] done by an earlier run"))
    }
}

trait Render: Send {
    fn event(&mut self, event: Event);
    fn tick(&mut self) {}
    fn finish(&mut self);
}

struct Quiet {
    paths: Paths,
    skipped: Skipped,
}

impl Render for Quiet {
    fn event(&mut self, event: Event) {
        if let Event::StepSkipped { index, of } = event {
            return self.skipped.add(index, of);
        }
        if let Some(line) = self.skipped.take() {
            eprintln!("{line}");
        }
        match event {
            Event::StepStart { index, of, label } => eprintln!("[{index}/{of}] {label}"),
            Event::StepEnd {
                index,
                of,
                label,
                ok,
                seconds,
            } => {
                let state = if ok { "done" } else { "failed" };
                eprintln!("[{index}/{of}] {label} {state} in {}", duration(seconds));
            }
            Event::PkgEnd {
                cpv,
                ok: false,
                failed_phase,
                log,
                ..
            } => eprintln!(
                "{}",
                self.paths
                    .failure(&cpv, failed_phase.as_deref(), log.as_deref())
            ),
            Event::PlanFailed { message } => eprintln!("{message}"),
            _ => {}
        }
    }

    fn finish(&mut self) {
        if let Some(line) = self.skipped.take() {
            eprintln!("{line}");
        }
    }
}

struct Running {
    cpv: String,
    bar: ProgressBar,
    log: Option<Utf8PathBuf>,
}

struct Session {
    job_id: String,
    total: ProgressBar,
    running: Vec<Running>,
}

struct Fancy {
    paths: Paths,
    multi: MultiProgress,
    steps: Option<ProgressBar>,
    session: Option<Session>,
    skipped: Skipped,
}

fn steps_style() -> ProgressStyle {
    ProgressStyle::with_template("[{bar:20.yellow/blue}] step {pos}/{len} {msg}")
        .expect("template")
        .progress_chars("=>-")
}

fn total_style() -> ProgressStyle {
    ProgressStyle::with_template("[{bar:30.green/blue}] {pos}/{len} packages {msg}")
        .expect("template")
        .progress_chars("=>-")
}

fn package_style() -> ProgressStyle {
    ProgressStyle::with_template("{spinner:.green} {elapsed:>4} {prefix}\n    {wide_msg:.dim}")
        .expect("template")
        .tick_chars("⠁⠂⠄⡀⢀⠠⠐⠈ ")
}

impl Fancy {
    fn steps(&mut self, of: u32) -> &ProgressBar {
        self.steps.get_or_insert_with(|| {
            let bar = self.multi.insert(0, ProgressBar::new(u64::from(of)));
            bar.set_style(steps_style());
            bar
        })
    }

    fn println(&self, line: &str) {
        let _ = self.multi.println(line);
    }

    fn session(&mut self, job_id: &str) -> Option<&mut Session> {
        self.session.as_mut().filter(|s| s.job_id == job_id)
    }

    fn end_session(&mut self) {
        if let Some(session) = self.session.take() {
            for running in session.running {
                running.bar.finish_and_clear();
                self.multi.remove(&running.bar);
            }
            session.total.finish_and_clear();
            self.multi.remove(&session.total);
        }
    }
}

impl Render for Fancy {
    fn event(&mut self, event: Event) {
        if let Event::StepSkipped { index, of } = event {
            self.steps(of).set_position(u64::from(index));
            return self.skipped.add(index, of);
        }
        if let Some(line) = self.skipped.take() {
            self.println(&line);
        }
        match event {
            Event::StepStart { index, of, label } => {
                let bar = self.steps(of);
                bar.set_position(u64::from(index - 1));
                bar.set_message(label);
            }
            Event::StepEnd {
                index,
                of,
                label,
                ok,
                seconds,
            } => {
                self.steps(of).set_position(u64::from(index));
                let state = if ok { "done" } else { "failed" };
                self.println(&format!(
                    "[{index}/{of}] {label} {state} in {}",
                    duration(seconds)
                ));
            }
            Event::SessionStart { job_id, plan_total } => {
                self.end_session();
                let total = self.multi.add(ProgressBar::new(u64::from(plan_total)));
                total.set_style(total_style());
                self.session = Some(Session {
                    job_id,
                    total,
                    running: Vec::new(),
                });
            }
            Event::SessionEnd { job_id } if self.session(&job_id).is_some() => self.end_session(),
            Event::PkgStart { job_id, cpv, log } => {
                let paths = self.paths.clone();
                let multi = self.multi.clone();
                if let Some(session) = self.session(&job_id) {
                    let bar = multi.add(ProgressBar::no_length());
                    bar.set_style(package_style());
                    bar.set_prefix(cpv.clone());
                    session.running.push(Running {
                        cpv,
                        bar,
                        log: log.map(|log| paths.host(&log)),
                    });
                }
            }
            Event::PhaseEnter { job_id, cpv, phase } => {
                if let Some(running) = self
                    .session(&job_id)
                    .and_then(|s| s.running.iter().find(|r| r.cpv == cpv))
                {
                    running.bar.set_prefix(format!("{cpv} {phase}"));
                }
            }
            Event::PkgEnd {
                job_id,
                cpv,
                ok,
                seconds,
                failed_phase,
                log,
            } => {
                let multi = self.multi.clone();
                if let Some(session) = self.session(&job_id) {
                    if let Some(at) = session.running.iter().position(|r| r.cpv == cpv) {
                        let running = session.running.remove(at);
                        running.bar.finish_and_clear();
                        multi.remove(&running.bar);
                    }
                    session.total.inc(1);
                }
                let line = if ok {
                    format!("  {cpv} {}", duration(seconds))
                } else {
                    self.paths
                        .failure(&cpv, failed_phase.as_deref(), log.as_deref())
                };
                self.println(&line);
            }
            Event::PlanFailed { message } => self.println(&message),
            _ => {}
        }
    }

    fn tick(&mut self) {
        if let Some(session) = &self.session {
            for running in &session.running {
                running.bar.tick();
                if let Some(line) = running.log.as_deref().and_then(|log| tail(log, 1).pop()) {
                    running.bar.set_message(line);
                }
            }
        }
    }

    fn finish(&mut self) {
        if let Some(line) = self.skipped.take() {
            self.println(&line);
        }
        self.end_session();
        if let Some(steps) = self.steps.take() {
            steps.finish_and_clear();
        }
    }
}

/// em's event stream for one command, drawn while it runs
pub struct Watch {
    /// Held open so the reader sees EOF only once em and this have both closed it
    keepalive: File,
    fifo: Utf8PathBuf,
    reader: JoinHandle<()>,
}

impl Watch {
    /// Create the FIFO in `log_dir` and start drawing what em writes to it
    pub fn start(
        sandbox_dir: &Utf8Path,
        log_dir: &Utf8Path,
        output: Output,
    ) -> std::io::Result<Self> {
        let fifo = log_dir.join(FIFO);
        let (read, keepalive) = open_fifo(&fifo)?;
        let paths = Paths {
            sandbox_dir: sandbox_dir.to_owned(),
            log_dir: log_dir.to_owned(),
        };
        let render: Box<dyn Render> = match output {
            Output::Fancy => Box::new(Fancy {
                paths,
                multi: MultiProgress::new(),
                steps: None,
                session: None,
                skipped: Skipped::default(),
            }),
            _ => Box::new(Quiet {
                paths,
                skipped: Skipped::default(),
            }),
        };
        let reader = std::thread::spawn(move || read_events(read, render));
        Ok(Self {
            keepalive,
            fifo,
            reader,
        })
    }

    /// Wait for the last event once em has exited
    pub fn finish(self) {
        drop(self.keepalive);
        let _ = self.reader.join();
        let _ = std::fs::remove_file(&self.fifo);
    }
}

/// A fresh FIFO at `path`: its blocking read end and a write end that keeps it from EOF
fn open_fifo(path: &Utf8Path) -> std::io::Result<(File, File)> {
    let _ = std::fs::remove_file(path);
    let c_path = std::ffi::CString::new(path.as_str())?;
    // SAFETY: `c_path` is a valid NUL-terminated path.
    if unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // Opening the read end non-blocking does not wait for a writer; reads then block again.
    let read = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)?;
    set_blocking(&read)?;
    let keepalive = std::fs::OpenOptions::new().write(true).open(path)?;
    Ok((read, keepalive))
}

fn set_blocking(file: &File) -> std::io::Result<()> {
    let fd = file.as_raw_fd();
    // SAFETY: `fd` is open for the duration of both calls.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags & !libc::O_NONBLOCK) } < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

fn read_events(read: File, mut render: Box<dyn Render>) {
    let fd = read.as_raw_fd();
    let mut reader = BufReader::new(read);
    let mut line = String::new();
    loop {
        if reader.buffer().is_empty() && !readable(fd, TICK) {
            render.tick();
            continue;
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if let Ok(event) = serde_json::from_str::<Event>(line.trim()) {
                    render.event(event);
                }
            }
        }
    }
    render.finish();
}

/// Whether `fd` has data (or EOF) within `timeout`
fn readable(fd: i32, timeout: Duration) -> bool {
    let mut poll = libc::pollfd {
        fd,
        events: libc::POLLIN,
        revents: 0,
    };
    // SAFETY: one valid pollfd.
    unsafe { libc::poll(&mut poll, 1, timeout.as_millis() as i32) > 0 }
}

/// The last `n` lines with text in the end of `log`, without escape sequences or `\r` redraws
fn tail(log: &Utf8Path, n: usize) -> Vec<String> {
    let Ok(mut file) = File::open(log) else {
        return Vec::new();
    };
    let len = file.metadata().map(|m| m.len()).unwrap_or(0);
    if file
        .seek(SeekFrom::Start(len.saturating_sub(16 * 1024)))
        .is_err()
    {
        return Vec::new();
    }
    let mut bytes = Vec::new();
    let _ = file.read_to_end(&mut bytes);
    let mut lines: Vec<String> = bytes
        .split(|&b| b == b'\n')
        .rev()
        .filter_map(|line| {
            line.split(|&b| b == b'\r')
                .rev()
                .map(visible_text)
                .find(|t| !t.is_empty())
        })
        .take(n)
        .collect();
    lines.reverse();
    lines
}

/// `bytes` as text without ANSI escape sequences and surrounding blanks
fn visible_text(bytes: &[u8]) -> String {
    let mut out = Vec::with_capacity(bytes.len());
    let mut iter = bytes.iter().copied().peekable();
    while let Some(b) = iter.next() {
        if b != 0x1b {
            if b >= 0x20 || b == b'\t' {
                out.push(b);
            }
            continue;
        }
        // CSI (`ESC [ ... final`) ends at a byte in 0x40..=0x7e; any other escape is two bytes.
        if iter.peek() == Some(&b'[') {
            iter.next();
            for c in iter.by_ref() {
                if (0x40..=0x7e).contains(&c) {
                    break;
                }
            }
        } else {
            iter.next();
        }
    }
    String::from_utf8_lossy(&out).trim().to_string()
}

fn duration(seconds: f64) -> String {
    let s = seconds.round() as u64;
    match (s / 3600, s / 60 % 60, s % 60) {
        (0, 0, s) => format!("{s}s"),
        (0, m, s) => format!("{m}m{s:02}s"),
        (h, m, _) => format!("{h}h{m:02}m"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::{Arc, Mutex};

    struct Record(Arc<Mutex<Vec<Event>>>);

    impl Render for Record {
        fn event(&mut self, event: Event) {
            self.0.lock().unwrap().push(event);
        }
        fn finish(&mut self) {}
    }

    #[test]
    fn events_decode_and_unknown_ones_are_skipped() {
        let lines = [
            r#"{"v":1,"event":"step_start","job_id":"r","index":2,"of":12,"label":"gcc-stage1","at":1.0}"#,
            r#"{"v":1,"event":"pkg_end","job_id":"s","cpv":"a/b-1","ok":false,"seconds":3.0,"failed_phase":"compile","extra":1}"#,
            r#"{"v":1,"event":"some_future_event","job_id":"r"}"#,
        ];
        let events: Vec<Event> = lines
            .iter()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        assert_eq!(
            events[0],
            Event::StepStart {
                index: 2,
                of: 12,
                label: "gcc-stage1".into()
            }
        );
        assert!(
            matches!(&events[1], Event::PkgEnd { ok: false, failed_phase: Some(p), log: None, .. } if p == "compile")
        );
        assert_eq!(events[2], Event::Other);
    }

    #[test]
    fn the_reader_drains_every_writer_before_it_stops() {
        let dir = std::env::temp_dir().join(format!("crossdev-stages-fifo-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let fifo = Utf8PathBuf::try_from(dir.join(FIFO)).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let (read, keepalive) = open_fifo(&fifo).unwrap();
        let record = Record(Arc::clone(&seen));
        let reader = std::thread::spawn(move || read_events(read, Box::new(record)));
        // Two writers in turn, as two em steps open and close the path; the first close is no EOF.
        for index in 1..=2 {
            let mut em = std::fs::OpenOptions::new()
                .append(true)
                .open(&fifo)
                .unwrap();
            writeln!(em, r#"{{"event":"step_skipped","index":{index},"of":2}}"#).unwrap();
        }
        drop(keepalive);
        reader.join().unwrap();
        assert_eq!(
            *seen.lock().unwrap(),
            [
                Event::StepSkipped { index: 1, of: 2 },
                Event::StepSkipped { index: 2, of: 2 }
            ]
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn skipped_steps_collapse_and_paths_map_to_the_host() {
        let mut skipped = Skipped::default();
        assert_eq!(skipped.take(), None);
        for index in 1..=11 {
            skipped.add(index, 12);
        }
        assert_eq!(
            skipped.take().as_deref(),
            Some("[1-11/12] done by an earlier run")
        );
        let paths = Paths {
            sandbox_dir: "/sb".into(),
            log_dir: "/logs/sb".into(),
        };
        assert_eq!(paths.host("/var/log/em.log"), "/logs/sb/em.log");
        assert_eq!(
            paths.host("/var/tmp/portage/a/b-1/temp/build.log"),
            "/sb/var/tmp/portage/a/b-1/temp/build.log"
        );
        assert_eq!(duration(192.4), "3m12s");
    }
}
