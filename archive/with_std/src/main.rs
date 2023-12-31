// NOTE
// This was my first try to find controllers and read their input
// This might even be a good way, simply because it does not require any crates and just uses std
// But doing it this way was quite tedious,
// its easier with hidapi and probably faster as well, because its at a lower level

//  build standalone binary for raspi:
// Tier 1 (guaranteed to work):     cargo build --target aarch64-unknown-linux-gnu          added to project
// Tier 2 (guaranteed to build):    cargo build --target armv7-unknown-linux-gnueabihf      added to project
// Tier 2                           cargo build --target armv7-unknown-linux-gnueabihf      not yet added to project, has linux kernel 4.15, instead of the v3.2 the option above has
// https://doc.rust-lang.org/nightly/rustc/platform-support.html
// Raspi Zero 2W has a 64-bit Arm Cortex-A53, which is ARMv8 64bit.. so the Tier 1 option should work

// Steps:
//  1. Connect Controller
//      - that is in pairing mode
//      - scan for controllers already known to the system
//  2. Read Controller Input
//      2.1 Find Controller(s) in list of devices
//      2.2 Read inputs (maybe two controllers can be multithreaded)
//      - linux command to compare results: evtest (non-root!)

mod finding_controllers;
mod read_input;
mod structs;

use crate::finding_controllers::*;
use crate::read_input::*;
use crate::structs::*;

use psutil::process::Process;
use std::{process, process::Command, thread, time::Duration};

fn main() {
    let mut loading_show: String = String::from("");
    let process = Process::new(process::id()).unwrap();
    let mut ctrls: GameControllerCollection = GameControllerCollection {
        first: None,
        second: None,
    };

    loop {
        get_and_insert_controllers(&mut ctrls);
        output_info(&mut loading_show, &process, &ctrls);
        handle_threads(&mut ctrls);

        // wait some time before checking for new devices
        thread::sleep(Duration::from_secs(2));
    }
}

/// - Checks the thread handle to see if all controllers have running threads
/// - Creates threads if necessary
/// - Takes care that no controller is assigned two or more threads
fn handle_threads(ctrls: &mut GameControllerCollection) {
    // Are there any controllers connected?
    match &mut ctrls.first {
        None => (),
        Some(ctrl) => _create_new_thread(ctrl),
    }
    match &mut ctrls.second {
        None => (),
        Some(ctrl) => _create_new_thread(ctrl),
    }

    // Create one thread per controller
    fn _create_new_thread(controller: &mut GameController) {
        match controller.thread_handle {
            None => {
                // Copy Controller for the new thread
                let threaded_controller: GameControllerSimple = GameControllerSimple {
                    path: controller.path.clone(),
                    mac: controller.mac.clone(),
                };

                // Create thread and link it in collection
                controller.thread_handle = Some(thread::spawn(move || {
                    read_controller_input(threaded_controller);
                }));
            }
            Some(_) => (),
        };
    }
}

/// Output Information to Terminal <br>
/// - Show that programm is active with little animation
/// - Show RAM usage
/// - Show connected controllers by their mac address
fn output_info(loading_show: &mut String, process: &Process, ctrls: &GameControllerCollection) {
    let _ = Command::new("clear").status();

    // show how if the programm is scanning for controllers or not
    match loading_show.len() < 10 {
        true => loading_show.push('.'),
        false => loading_show.clear(),
    }

    // show memory usage
    let memory_usage = process.memory_info().unwrap().rss() as f64 / (1024.0 * 1024.0);
    println!("Memory usage: {:.2} MB {:}", memory_usage, loading_show);
    println!("");

    // Show what is connected
    match &ctrls.first {
        None => (),
        Some(ctrl) => {
            println!("{:?} connected", ctrl.mac.clone());
        }
    }
    match &ctrls.second {
        None => (),
        Some(ctrl) => {
            println!("{:?} connected", ctrl.mac.clone());
        }
    }
}
