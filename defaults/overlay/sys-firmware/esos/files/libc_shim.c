/*
 * libc_shim.c — RT-Thread libc replacement for K3 ESOS (USE=k3 only).
 *
 * K3 RT24 entry is at 0x100200000 (>4 GiB) so the ESOS link uses -mcmodel=medany.
 * Arch's riscv64-elf-newlib is built with medlow, so newlib symbol relocations
 * (R_RISCV_HI20) overflow the 32-bit immediate when linked into a >4 GiB image.
 *
 * Solution: drop newlib via `-nostdlib -lgcc` in K3 rtconfig.py LFLAGS, and
 * shim the handful of libc functions ESOS calls onto RT-Thread's rt_* APIs.
 */

#include <rtthread.h>
#include <stddef.h>
#include <stdarg.h>

void *memset(void *s, int c, size_t n)         { return rt_memset(s, c, n); }
void *memcpy(void *d, const void *s, size_t n) { return rt_memcpy(d, s, n); }
void *memmove(void *d, const void *s, size_t n){ return rt_memmove(d, s, n); }
int   memcmp(const void *a, const void *b, size_t n) { return rt_memcmp(a, b, n); }
void *memchr(const void *s, int c, size_t n)
{
	const unsigned char *p = s; unsigned char ch = (unsigned char)c;
	while (n--) { if (*p == ch) return (void *)p; p++; }
	return NULL;
}

size_t strlen(const char *s)                   { return rt_strlen(s); }
size_t strnlen(const char *s, size_t m)        { return rt_strnlen(s, m); }
int    strcmp(const char *a, const char *b)    { return rt_strcmp(a, b); }
int    strncmp(const char *a, const char *b, size_t n) { return rt_strncmp(a, b, n); }
char  *strncpy(char *d, const char *s, size_t n)       { return rt_strncpy(d, s, n); }
char  *strcpy(char *d, const char *s)
{
	char *r = d;
	while ((*d++ = *s++));
	return r;
}
char  *strcat(char *d, const char *s)
{
	char *r = d;
	while (*d) d++;
	while ((*d++ = *s++));
	return r;
}
char  *strstr(const char *a, const char *b)    { return rt_strstr(a, b); }

void  *malloc(size_t n)                        { return rt_malloc(n); }
void  *calloc(size_t a, size_t b)              { return rt_calloc(a, b); }
void  *realloc(void *p, size_t n)              { return rt_realloc(p, n); }
void   free(void *p)                           { rt_free(p); }

int    abs(int x)                              { return x < 0 ? -x : x; }
int    atoi(const char *s)
{
	int sign = 1, v = 0;
	if (!s) return 0;
	while (*s == ' ' || *s == '\t') s++;
	if (*s == '-') { sign = -1; s++; }
	else if (*s == '+') s++;
	while (*s >= '0' && *s <= '9') { v = v * 10 + (*s - '0'); s++; }
	return v * sign;
}
int    ffs(int x) { return x ? __builtin_ffs(x) : 0; }
int    fls(int x)
{
	int r = 0;
	if (!x) return 0;
	while (x) { x = (unsigned)x >> 1; r++; }
	return r;
}

int    printf(const char *f, ...)
{
	va_list ap;
	char buf[256];
	va_start(ap, f);
	rt_vsnprintf(buf, sizeof(buf), f, ap);
	va_end(ap);
	rt_kprintf("%s", buf);
	return rt_strlen(buf);
}

void __assert_func(const char *file, int line, const char *func, const char *expr)
{
	rt_kprintf("assert: %s:%d: %s: %s\n", file, line, func ? func : "?", expr);
	while (1) { }
}
