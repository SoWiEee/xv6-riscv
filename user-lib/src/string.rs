// user-lib/src/string.rs
pub fn strlen(s: &str) -> usize {
    s.len()
}

pub fn strcmp(a: &str, b: &str) -> i32 {
    a.cmp(b) as i32
}

pub fn strcpy<'a>(dst: &'a mut [u8], src: &str) -> &'a mut [u8] {
    let len = src.len().min(dst.len() - 1);
    dst[..len].copy_from_slice(&src.as_bytes()[..len]);
    dst[len] = 0;
    dst
}

pub fn strncpy<'a>(dst: &'a mut [u8], src: &str, n: usize) -> &'a mut [u8] {
    let len = src.len().min(n).min(dst.len());
    dst[..len].copy_from_slice(&src.as_bytes()[..len]);
    if len < dst.len() {
        dst[len] = 0;
    }
    dst
}