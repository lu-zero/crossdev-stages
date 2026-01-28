fn main() {
    let filename = "stage3-riscv64-openrc-20231018T010001Z.tar.xz";
    let parts: Vec<&str> = filename.split('-').collect();
    println!("Filename: {}", filename);
    println!("Parts: {:?}", parts);
    println!("Parts len: {}", parts.len());
    if parts.len() >= 4 {
        let timestamp_part = parts[parts.len() - 2];
        println!("Timestamp part: {}", timestamp_part);
        if timestamp_part.len() >= 8 {
            let date = &timestamp_part[..8];
            println!("Date: {}", date);
        }
    }
}
