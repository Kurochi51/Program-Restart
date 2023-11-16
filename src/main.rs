use std::{io, thread};
use std::io::{BufRead, Write};
use std::path::MAIN_SEPARATOR;
use std::process::{Command, Stdio};
use std::sync::{Arc, Condvar, mpsc, Mutex};
use std::time::{Duration, Instant};
use chrono::{Local, Timelike};

use term_cursor as cursor;
use tasklist;
use tokio::time;

fn main()
{
    // Prompt the user to input the process name.
    println!("Enter the name of the process you want to search for:");
    let name = get_user_input();
    let process_name = format!("{}.exe", name.trim());
    let mut app = String::new();
    if let Some(process_location) = find_process_by_name(&process_name)
    {
        let location = process_location.as_str();

        if let Some(last_separator_index) = location.rfind(MAIN_SEPARATOR)
        {
            let result = &location[(last_separator_index + 1)..];
            app = result.to_string();
        }
    } else
    {
        println!("Process not found.");
        println!("Press any key to exit...");
        get_user_input();
        return;
    }

    // Prompt the user to input the restart interval.
    let restart_interval_hours: i32;
    println!("Enter the restart interval in hours:");
    loop
    {
        match get_user_input().parse::<i32>()
        {
            Ok(input) =>
                {
                    restart_interval_hours = input * 60 * 60;
                    break;
                }
            Err(_) => println!("Invalid input. Please enter a valid number."),
        };
    }
    let restart_interval = Duration::from_secs(restart_interval_hours as u64);
    let restart_time = Instant::now() + restart_interval;
    let system_restart_time = Local::now() + restart_interval;
    println!("Restart occurs at: {:0>2}:{:0>2}:{:0>2}", system_restart_time.time().hour(), system_restart_time.time().minute(), system_restart_time.time().second());

    let exit_signal = Arc::new((Mutex::new(false), Condvar::new()));

    let exit_signal_clone = exit_signal.clone();
    let secondary_exit_signal = exit_signal.clone();
    let monitor_thread = thread::spawn(move || monitor_thread(exit_signal_clone, process_name, restart_interval_hours));

    println!("Application: {}", app);
    println!("1. Exit");

    let timer_thread = thread::spawn( move || timer_thread(secondary_exit_signal, restart_interval, restart_time));

    loop
    {
        let (lock, _cvar) = &*exit_signal;
        let monitoring_thread_exited = *lock.lock().unwrap();

        if monitoring_thread_exited
        {
            break;
        }
        let timeout_duration = Duration::from_millis(2000);
        let input = get_user_input_with_timeout(timeout_duration);
        match input
        {
            Some(string) => match string.trim()
            {
                "1" =>
                    {
                        let (lock, cvar) = &*exit_signal;
                        *lock.lock().unwrap() = true;
                        cvar.notify_all();
                        println!("Exiting program.");
                        break;
                    }
                "" =>
                    {
                        println!("No option selected. Please select a valid option.");
                        println!("1. Exit");
                    }
                _ =>
                    {
                        println!("Invalid option {}. Please select a valid option.", string);
                        println!("1. Exit");
                    }
            },
            None => continue,
        }
    }

    if let Err(err) = monitor_thread.join()
    {
        eprintln!("Error in the monitoring thread: {:?}", err);
    }
    if let Err(err) = timer_thread.join()
    {
        eprintln!("Error in the timer thread: {:?}",err);
    }
    println!("Press any key to exit...");
    get_user_input();
}

fn get_user_input() -> String
{
    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .expect("Failed to read input");
    input.trim().to_string()
}

fn get_user_input_with_timeout(timeout: Duration) -> Option<String>
{
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut input = String::new();
        let stdin = io::stdin();
        let mut handle = stdin.lock();

        if handle.read_line(&mut input).is_ok()
        {
            tx.send(input.trim().to_string()).ok();
        }
    })
        .join()
        .expect("Error in input thread joining");
    let result = rx.recv_timeout(timeout);
    match result
    {
        Err(_) => None,
        Ok(input) => Some(input),
    }
}

fn find_process_by_name(process_name: &str) -> Option<String>
{
    unsafe
        {
            let list = tasklist::Tasklist::new();
            for process in list {
                if process.pname.as_str().to_lowercase() == process_name.to_lowercase()
                {
                    return Some(process.get_path().to_string());
                }
            }
            None
        }
}

unsafe fn restart_process(executable_path: &str, process_name: &str) -> Result<(), String>
{
    let pid = find_pid(process_name);

    if pid == 0 {
        let message = format!("Program {} isn't running.", process_name);
        return Err(message);
    }

    let kill_command = format!("taskkill /F /PID {}", pid);

    match Command::new("cmd")
        .arg("/C")
        .arg(&kill_command)
        .stdout(Stdio::null())
        .status()
    {
        Ok(status) =>
            {
                if !status.success()
                {
                    eprintln!("Error terminating process with PID: {}", pid);
                }
            }
        Err(err) =>
            {
                eprintln!("Error executing command: {}", err);
            }
    }
    let last_separator_index = executable_path.rfind(MAIN_SEPARATOR).unwrap_or(0);
    let working_dir = &executable_path[..last_separator_index];
    match Command::new(executable_path)
        .current_dir(working_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => Ok(()),
        Err(err) => Err(format!("Failed to restart process: {}", err)),
    }
}

unsafe fn find_pid(process_name: &str) -> u32
{
    let list = tasklist::Tasklist::new();
    for process in list
    {
        if process.pname.as_str().to_lowercase() == process_name.to_lowercase()
        {
            return process.pid;
        }
    }
    0
}

fn timer_thread(exit_signal_clone: Arc<(Mutex<bool>, Condvar)>, restart_interval: Duration, restart_time: Instant)
{
    let origin_cursor = cursor::get_pos().unwrap();
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(async {
        tokio::task::spawn(async move {
            let mut interval = time::interval(Duration::from_secs(1));
            let mut restart_time = restart_time;
            loop
            {
                let (lock, _cvar) = &*exit_signal_clone;
                let should_exit = *lock.lock().unwrap();

                if should_exit
                {
                    break;
                }

                let time_remaining = restart_time.duration_since(Instant::now());
                if time_remaining.as_secs() <= 1
                {
                    restart_time = Instant::now() + restart_interval;
                }
                let seconds = time_remaining.as_secs() % 60;
                let minutes = (time_remaining.as_secs() / 60) % 60;
                let hours = (time_remaining.as_secs() / 60) / 60;
                let current_cursor = cursor::get_pos().unwrap();
                if origin_cursor != current_cursor
                {
                    let _ = cursor::set_pos(origin_cursor.0, origin_cursor.1);
                    print!("\rTime until restart: {:0>2}:{:0>2}:{:0>2}", hours, minutes, seconds);
                    let _ = cursor::set_pos(current_cursor.0, current_cursor.1);
                } else {
                    let _ = cursor::set_pos(origin_cursor.0, origin_cursor.1);
                    print!("\rTime until restart: {:0>2}:{:0>2}:{:0>2}", hours, minutes, seconds);
                    let _ = cursor::set_pos(current_cursor.0, current_cursor.1 + 1);
                }
                io::stdout().flush().unwrap();
                interval.tick().await;
            }
            let (lock, cvar) = &*exit_signal_clone;
            *lock.lock().unwrap() = true;
            cvar.notify_all();
        }).await.unwrap();
    });
}

fn monitor_thread(exit_signal_clone: Arc<(Mutex<bool>,Condvar)>, process_name: String, restart_interval_hours: i32)
{
    loop
    {
        // Check the exit signal.
        let (lock, cvar) = &*exit_signal_clone;
        let should_exit = *lock.lock().unwrap();

        if should_exit
        {
            break;
        }

        if let Some(process_location) = find_process_by_name(&process_name)
        {
            // Sleep with the ability to be interrupted.
            let result = cvar
                .wait_timeout_while(lock.lock().unwrap(), Duration::from_secs(restart_interval_hours as u64), |&mut should_exit| !should_exit)
                .unwrap();

            unsafe
                {
                    if result.1.timed_out()
                    {
                        if let Err(err) = restart_process(&process_location, &process_name)
                        {
                            eprintln!("Failed to restart process: {}", err);
                        }
                    }
                }
        } else
        {
            eprintln!("Process {} not found. Exiting...", process_name);
            break;
        }
    }
    let (lock, cvar) = &*exit_signal_clone;
    *lock.lock().unwrap() = true;
    cvar.notify_all();
}