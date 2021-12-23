use crate::{sys, usr};
use crate::api::console::Style;
use alloc::string::ToString;

pub fn main(args: &[&str]) -> usr::shell::ExitCode {
    if args.len() == 1 || args[1] == "usage" {
        usage()
    } else if args[1] == "format" {
        sys::fs::mount_mem();
        sys::fs::format_mem();
        println!("Memory successfully formatted");
        println!("MFS is now mounted to '/'");
        usr::shell::ExitCode::CommandSuccessful
    } else {
        help();
        usr::shell::ExitCode::CommandError
    }
}

fn usage() -> usr::shell::ExitCode {
    let size = sys::allocator::memory_size();
    let used = sys::allocator::memory_used();
    let free = size - used;

    let width = size.to_string().len();
    let color = Style::color("LightCyan");
    let reset = Style::reset();
    println!("{}Size:{} {:width$}", color, reset, size, width = width);
    println!("{}Used:{} {:width$}", color, reset, used, width = width);
    println!("{}Free:{} {:width$}", color, reset, free, width = width);
    usr::shell::ExitCode::CommandSuccessful
}

fn help() -> usr::shell::ExitCode {
    let csi_option = Style::color("LightCyan");
    let csi_title = Style::color("Yellow");
    let csi_reset = Style::reset();
    println!("{}Usage:{} memory {}<command>{}", csi_title, csi_reset, csi_option, csi_reset);
    println!();
    println!("{}Commands:{}", csi_title, csi_reset);
    println!("  {}usage{}     List memory usage", csi_option, csi_reset);
    println!("  {}format{}    Format RAM disk", csi_option, csi_reset);
    usr::shell::ExitCode::CommandSuccessful
}
