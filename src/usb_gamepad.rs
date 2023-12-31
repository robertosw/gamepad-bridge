use flume::Receiver;
use flume::TryRecvError;
use std::env;
use std::fs::File;
use std::{io::Write, process::exit, thread, time::Instant};
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use crate::usb_gamepad_ps4::DUALSHOCK;
use crate::usb_gamepad_ps5::DUALSENSE;
use crate::{print_error_and_exit, universal_gamepad::UniversalGamepad, usb_gadget::UsbGadgetDescriptor};

pub const OUTPUT_GAMEPADS: [&Gamepad; 2] = [&DUALSENSE, &DUALSHOCK];

pub struct Gamepad {
    pub gadget: UsbGadgetDescriptor,

    /// This depends on how the function bt_input_to_universal_gamepad() works
    /// Currently this is used to be sure that at least all standard buttons, triggers, bumpers etc are readable
    pub min_bt_report_size: usize,

    /// Is this gamepad fully usable as an output gamepad
    pub is_supported: bool,

    /// Used for verbose output
    pub display_name: &'static str,

    /// what strings can a user input as the second commandline argument to select this gamepad for use as the output gamepad
    pub associated_args: [&'static str; 2],
    pub bt_input_to_universal_gamepad: fn(&Vec<u8>) -> UniversalGamepad,
    pub universal_gamepad_to_usb_output: fn(&UniversalGamepad) -> Vec<u8>,
}
impl Gamepad {
    /// Checks if there has been one command line argument given, exits with descriptive error if not
    ///
    /// If argument was given, checks if it contains a string describing any supported gamepad
    pub fn from_cmdline_args() -> &'static Gamepad {
        let args: Vec<String> = env::args().collect();

        if args.len() != 2 {
            println!("One command line argument was expected to describe the desired output gamepad");
            println!("If run with cargo, use: cargo run -- <argument>");
            _display_supported_gamepads();
        }

        let given_arg: &String = &args[1];

        for gamepad in OUTPUT_GAMEPADS {
            for associated_arg in gamepad.associated_args {
                if given_arg.contains(associated_arg) {
                    if gamepad.is_supported {
                        println!("Output gamepad is {}", gamepad.display_name);
                        return gamepad;
                    } else {
                        println!("The gamepad {} is not yet supported", gamepad.display_name);
                        break;
                    }
                }
            }
        }

        println!("No supported gamepad is associated with the input '{}'", given_arg);
        _display_supported_gamepads();
    }

    pub fn bt_input_to_universal_gamepad(&self, bt_input: &Vec<u8>) -> UniversalGamepad {
        return (self.bt_input_to_universal_gamepad)(&bt_input);
    }

    /// - Waits for a new UniversalGamepad, evaluating only the latest message in the channel, exits automatically if the channel is closed
    /// - Transforms the given `UniversalGamepad` into the correct output array for this `Gamepad`
    /// - Attempts to write the entire output array into the file /dev/hidg0
    pub fn write_to_gadget_continously(&self, receiver: Receiver<UniversalGamepad>) {
        let start_instant: Instant = Instant::now();

        for gamepad in receiver.iter() {
            let msg_count = receiver.len();
            if msg_count > 5 {
                let all_msgs = receiver.drain().enumerate();

                for (index, _) in all_msgs {
                    if index + 1 < msg_count {
                        continue;
                    }
                }
                println!("skipped {msg_count} inputs {:?} after launch", (Instant::now() - start_instant));
            } else if msg_count > 1 {
                println!("skipped 1 input {:?} after launch", (Instant::now() - start_instant));
                continue; // take only the latest inputs
            }

            let usb_output: Vec<u8> = self.universal_gamepad_to_usb_output(&gamepad);

            let mut hidg0 = match File::options().write(true).append(false).open("/dev/hidg0") {
                Ok(file) => file,
                Err(err) => print_error_and_exit!("Could not open file hidg0", err, 1),
            };

            match hidg0.write_all(&usb_output) {
                Ok(_) => (),
                Err(err) => println!("write to hidg0 failed: {:?}", err),
            }
        }
    }

    /// To adjust how precice the interval has to be "hit", `max_deviation` can be used.
    ///
    /// - `max_deviation`:
    ///     - using `1` would mean, that the interval is practically disregarded and writing to file is done as soon as possible
    ///     - using `0` would nearly never write to file, because this loop itself has a bit of runtime and never hits the interval with 0ns accuracy
    ///     - useful values are
    ///         - `~ 0.05`, if its important that writing is done nearly every interval
    ///         - `< 0.001`, if its important that the interval is precicely hit
    pub fn write_to_gadget_intervalic(&self, universal_gamepad: Arc<Mutex<UniversalGamepad>>, interval: Duration, max_deviation: f32, recv: Receiver<bool>) {
        // its safe to use u128 for nanoseconds
        // 2^64 ns are ~580 years
        // so 2^128 are 580² years

        let start: Instant = Instant::now();
        let interval_ns = interval.as_nanos();
        let mut interval_counts_before: u128 = 0;

        let mut code_ran: bool = false;
        let mut usb_output: Vec<u8> = Vec::new();

        loop {
            match recv.try_recv() {
                Ok(_) | Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => (),
            }
            {
                if code_ran == false {
                    // program code that might not run fast enough for interval here
                    let gamepad_locked = universal_gamepad.lock().expect("Locking Arc<Mutex<UniversalGamepad>> failed!");
                    usb_output = self.universal_gamepad_to_usb_output(&gamepad_locked);
                }
                code_ran = true;
            }

            // Is this loop still "on time"?
            let now: Instant = Instant::now();
            let elapsed_ns: u128 = (now - start).as_nanos();
            let interval_counts_now: u128 = elapsed_ns / interval_ns;

            // By how many ns does the current run deviate from the cycle
            let diff_from_interval_ns: u128 = elapsed_ns % interval_ns;

            let is_next_interval: bool = interval_counts_now > interval_counts_before;
            let is_close_enough: bool = (diff_from_interval_ns as f32 / interval_ns as f32) <= max_deviation;

            if is_next_interval && is_close_enough {
                {
                    // code that is supposed to be timed, herek
                    let mut hidg0 = match File::options().write(true).append(false).open("/dev/hidg0") {
                        Ok(file) => file,
                        Err(err) => print_error_and_exit!("Could not open file hidg0", err, 1),
                    };

                    match hidg0.write_all(&usb_output) {
                        Ok(_) => (),
                        Err(err) => println!("write to hidg0 failed: {:?}", err),
                    }
                }

                interval_counts_before = interval_counts_now;
                code_ran = false;
                usb_output.clear();
            }

            thread::sleep(Duration::from_nanos(1));
        }
    }

    pub fn debug_output_bt_input(&self, gamepad: &UniversalGamepad) {
        print!("{}", termion::clear::All);
        print!("{}", termion::cursor::Goto(1, 1));
        println!(
            "Lx:{:5?}\tLy:{:5?}\tL: {:5?}\tRx:{:5?}\tRy:{:5?}\tR: {:5?}",
            gamepad.sticks.left.x,
            gamepad.sticks.left.y,
            gamepad.sticks.left.pressed,
            gamepad.sticks.right.x,
            gamepad.sticks.right.y,
            gamepad.sticks.right.pressed,
        );

        print!("{}", termion::cursor::Goto(1, 2));
        println!(
            "Tl:{:5?}\tTr:{:5?}\tBl:{:?}\tBr:{:?}",
            gamepad.triggers.left, gamepad.triggers.right, gamepad.buttons.bumpers.left, gamepad.buttons.bumpers.right,
        );

        print!("{}", termion::cursor::Goto(1, 3));
        println!(
            "X: {:5?}\tO: {:5?}\t□: {:5?}\t∆: {:5?}",
            gamepad.buttons.main.lower, gamepad.buttons.main.right, gamepad.buttons.main.left, gamepad.buttons.main.upper
        );

        print!("{}", termion::cursor::Goto(1, 4));
        println!(
            "↑: {:5?}\t→: {:5?}\t↓: {:5?}\t←: {:5?}",
            gamepad.buttons.dpad.up, gamepad.buttons.dpad.right, gamepad.buttons.dpad.down, gamepad.buttons.dpad.left
        );

        print!("{}", termion::cursor::Goto(1, 5));
        println!(
            "S: {:5?}\tM: {:5?}\tLogo: {:5?}",
            gamepad.buttons.specials.left, gamepad.buttons.specials.right, gamepad.buttons.specials.logo
        );
    }

    /// creates a `Vec<u8>` that is the HID Report which has to be written in `/dev/hidg0`
    ///
    /// The length will be asserted at runtime to be `self.gadget.functions_hid.report_length`. This function will **panic** if the length is not correct
    pub fn universal_gamepad_to_usb_output(&self, gamepad: &UniversalGamepad) -> Vec<u8> {
        return (self.universal_gamepad_to_usb_output)(gamepad);
    }
}

fn _display_supported_gamepads() -> ! {
    println!("");
    println!("Supported gamepads are:");
    for gamepad in OUTPUT_GAMEPADS {
        if gamepad.is_supported {
            println!("{}: with any of {:?} as the argument", gamepad.display_name, gamepad.associated_args);
        }
    }
    exit(1);
}
