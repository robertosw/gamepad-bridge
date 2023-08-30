#![allow(dead_code, unreachable_code)]

#[macro_use]
extern crate version;
// To allow using the version! macro

use ctrlc::set_handler;
use hidapi::HidApi;
use std::env;
use std::fs::File;
use std::io::Read;
use std::process::exit;
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

mod bluetooth_fn;
mod helper_fn;
mod hidapi_fn;
mod hidapi_read_ps5_usb;
mod hidapi_structs;
mod usb_gadget;
mod usb_gamepads;

use crate::bluetooth_fn::*;
use crate::hidapi_fn::*;

// build, then run as root to allow hidraw read
// clear && cargo build --release && sudo chown root:root target/release/gamepad-bridge && sudo chmod +s target/release/gamepad-bridge && target/release/gamepad-bridge

fn main() {
    println!("\nGamepad-Bridge started: v{:}", version!());
    println!("This program requires root privilages. Please set uuid accordingly.\n");

    // TODO next steps: 
    // 1. generalize the read_ps5_usb function to read from some device 
    //    and show the output formatted with each byte to 3 digits
    // 2. create struct which describes intepretation of input data per gamepad

    let api = match HidApi::new() {
        Ok(api) => api,
        Err(err) => {
            println!("Error getting HidApi access: {:?}", err);
            exit(2);
        }
    };

    let _gamepads: Vec<hidapi::DeviceInfo> = find_supported_gamepads(api);
    for gamepad in _gamepads {
        let api = match HidApi::new() {
            Ok(api) => api,
            Err(err) => {
                println!("Error getting HidApi access: {:?}", err);
                exit(2);
            }
        };
        open_device(gamepad, api);
    }

    // PS5_GAMEPAD.configure_device();
    // PS4_GAMEPAD.configure_device();
    // GENERIC_KEYBOARD.configure_device();

    exit(0);

    // TODO Check if hidg0 device exists

    // TODO Retry hidg crate

    // TODO Write to hidg0 device manually (example)
    // sudo su
    // echo -ne "\0\0\x4\0\0\0\0\0" > /dev/hidg0 #press the A-button
    // echo -ne "\0\0\0\0\0\0\0\0" > /dev/hidg0 #release all keys
}

// Ideas for program flow
// 1. the whole procedure (BT finding, input read, output to usb) is being duplicated for each player right inside main. So 1-4 threads
//     Problem: two threads could use the same gamepad and think its their own.. so output duplication
// 2. the bluetooth scanning is one thread, seperate from main (output written in file or shared mem)
//     - output is interpreted inside main thread
//     after an active device is connected, only then is a thread spawned for this device
//     -> threads dont have to know from each others existence (maybe for usb output, but we'll see)

fn _bt_program_flow() {
    // Create a shared boolean flag to indicate if Ctrl+C was pressed
    let ctrlc = Arc::new(AtomicBool::new(true));
    let ctrlc_clone = ctrlc.clone();

    // Set the flag to false when Ctrl+C is pressed
    match set_handler(move || ctrlc_clone.store(false, Ordering::SeqCst)) {
        Ok(_) => (),
        Err(err) => {
            println!("Error setting Ctrl-C handler {:?}", err);
            exit(1);
        }
    };

    bt_power_on();
    let (shared_mem_scan_output, thread_handle) = bt_scan_on_threaded();

    // find new controllers
    // loop while ctrlc has not been pressed (.load == true)
    while ctrlc.load(Ordering::SeqCst) {
        handle_bt_scan_output(&shared_mem_scan_output);

        thread::sleep(Duration::from_millis(500));
    }

    thread_handle.join().unwrap();
}

fn _hidapi_starter() {
    let api = match HidApi::new() {
        Ok(api) => api,
        Err(err) => {
            println!("Error getting HidApi access: {:?}", err);
            exit(2);
        }
    };

    let _gamepads: Vec<hidapi::DeviceInfo> = find_supported_gamepads(api);
}

fn read_uinput() {
    // The main problem is that the same device has a different report descriptor over bt and usb
    // so knowing the report descriptor from usb is quite useless for understanding the raw bt input

    /* Find out which device is which hidraw:
    loop over all hidraw devices and read this file:

    cat /sys/class/hidraw/hidrawX/device/uevent
        DRIVER=sony
        HID_ID=0005:0000054C:000009CC
        HID_NAME=Wireless Controller
        ...

    But in the end, hidapi also reads from hidrawX so why implement this again?
    */

    // let mut buf: [u8; 100] = [0; 100];

    let mut file = match File::options().read(true).open("/dev/hidraw3") {
        Ok(file) => file,
        Err(err) => {
            println!("could not open: {:?}", err);
            exit(1);
        }
    };

    loop {
        let mut buf: Vec<u8> = Vec::new();
        match file.read_to_end(&mut buf) {
            // Ok(i) => println!("read {i} bytes"),
            Ok(_) => (),
            Err(err) => println!("Read failed: {:?}", err),
        };
        let _ = Command::new("clear").status();

        for byte in buf {
            print!("{:03} ", byte);
        }
        println!();
        thread::sleep(Duration::from_millis(3));
    }
}
