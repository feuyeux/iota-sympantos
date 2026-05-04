const random_size = @import("random_size.zig");

pub extern "c" fn write(fd: c_int, buf: *const anyopaque, count: usize) isize;

pub fn main() void {
    const val = random_size.randomSize();
    _ = write(1, val.ptr, val.len);
    _ = write(1, "\n", 1);
}
