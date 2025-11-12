mod app;

use std::fs::File;
use std::path::PathBuf;
use std::time::Duration;
use clap::{Arg, ArgAction, Command, arg, command, value_parser};
use lldb::{lldb_addr_t, SBAddress, SBEvent, SBLaunchInfo, SBListener, SBProcess, StopReason};
use memchr::memmem;
use memmap2::Mmap;
use snafu::prelude::*;
use sysinfo::ProcessesToUpdate;

#[derive(Debug, Snafu)]
pub enum Error {
    #[snafu(context(false))]
    Underlying { source: ntdb_unwrap::Error },
    #[snafu(context(false))]
    IO { source: std::io::Error },
    #[snafu()]
    Sqlite { source: rusqlite::Error, op: String },
    #[snafu(whatever, display("{message}"))]
    App { message: String },
}
pub type Result<T> = std::result::Result<T, Error>;
fn main() -> Result<()> {
    let file = File::open("/Applications/QQ.app/Contents/Resources/app/wrapper.node")?;
    let mmap = unsafe { Mmap::map(&file)? };
    let pattern_bytes = "nt_sqlite3_key_v2:".as_bytes();
        assert_eq!(0xCAFEBABE, u32::from_be_bytes([
            mmap[0],
            mmap[1],
            mmap[2],
            mmap[3]
        ]));
        let fat_bin_offset = fat_macho::FatReader::new(&mmap[0..1024]).unwrap().find_cputype(
            if cfg!(target_arch = "aarch64") {
                0x0100000c
            } else {
                0x01000007
            }
        ).unwrap().unwrap().offset as usize;
        println!("offset: {fat_bin_offset}", );

    if let Some(index) = memmem::find(&mmap[fat_bin_offset..], pattern_bytes) {
        println!("Pattern found at byte offset: {:x}", index);
        println!("{}", String::from_utf8_lossy(&mmap[fat_bin_offset + index..fat_bin_offset + index + pattern_bytes.len() + 9]));
        let inpage_offset = index & 0xfff;
        let command = (0x91u32 << 24) | ((inpage_offset as u32) << 10) | (1 << 5) | 1;
        println!("Command: {:x}", command);
        let mut cmds = Vec::<(usize, u32)>::new(); // (occurrence, prev_cmd)
        for occurrence in memmem::find_iter(&mmap[fat_bin_offset..], &command.to_le_bytes()) {
            if mmap[occurrence + fat_bin_offset - 1] & 0xf0 == 0xf0 {
                let prev_cmd = u32::from_le_bytes([
                    mmap[fat_bin_offset + occurrence - 4],
                    mmap[fat_bin_offset + occurrence - 3],
                    mmap[fat_bin_offset + occurrence - 2],
                    mmap[fat_bin_offset + occurrence - 1],
                ]);
                cmds.push((occurrence, prev_cmd));
            }
        }
        println!("Found {} matches", cmds.len());
        let adrp = if cmds.len() > 1 {
            println!("Warning: multiple matches found, using the last one.");
            cmds.last().unwrap().0
        } else {
            cmds.first().unwrap().0
        };
        let func_start = memmem::rfind(&mmap[fat_bin_offset..fat_bin_offset + adrp], &0xD10103FFu32.to_le_bytes()).unwrap();

        let mut sysinf = sysinfo::System::new();
        sysinf.refresh_processes(ProcessesToUpdate::All, true);
        if sysinf.processes().iter().any(|x| {
            x.1.exe() == Some(&PathBuf::from("/Applications/QQ.app/Contents/MacOS/QQ"))
        }) {
            println!("QQ正在运行，请先退出");
            return Ok(());
        }
        lldb::SBDebugger::initialize();
        let debugger = lldb::SBDebugger::create(false);
        debugger.set_asynchronous(true);
        let target = debugger.create_target_simple("/Applications/QQ.app/Contents/MacOS/QQ").unwrap();
        // // 4. 设置断点
        // let breakpoint = target
        //     .breakpoint_create_by_sbaddress(breakpoint_function_name, None)?;
        // if !breakpoint.is_valid() {
        //     panic!("Failed to set breakpoint at '{}'", breakpoint_function_name);
        // }
        // println!("Breakpoint set at '{}'", breakpoint_function_name);

        let launch_info = SBLaunchInfo::new();
        // 7. 启动进程
        // 注意：我们将 listener 传递给 launch，以便在进程启动时就开始监听
        let process = target.launch(launch_info).unwrap();
        if !process.is_valid() {
            panic!("Failed to launch process");
        }
        println!("Process launched (PID: {}).", process.process_id());

        let mut dylib_base_addr = 0;
        loop {
            let result = debugger.execute_command("image list -o -f | grep /Applications/QQ.app/Contents/Resources/app/wrapper.node");
            match result {
                Ok(str) => {
                    if str.contains("wrapper.node") {
                        let num = &str[
                            str.find("0x").unwrap() + 2..
                                str.rfind(" /Application").unwrap()
                            ];
                        println!("num {}", num);
                        if let Ok(addr) = u64::from_str_radix(num, 16) {
                            dylib_base_addr = addr;
                            println!("模块加载完成 {dylib_base_addr:x}");
                            let breakpoint_addr = dylib_base_addr + func_start as u64;
                            let breakpoint = target
                                .breakpoint_create_by_sbaddress(SBAddress::from_load_address(breakpoint_addr, &target));
                            if !breakpoint.is_valid() {
                                panic!("Failed to set breakpoint at 0x{breakpoint_addr:x}");
                            }
                            println!("Breakpoint set at 0x{breakpoint_addr:x}");
                            break;
                        }
                    }
                }
                Err(..) => {
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        'event_loop: loop {
            // 等待事件
            let event = SBEvent::new();
            if !debugger.listener().wait_for_event(10, &event) {
                println!("timeout waiting for event...");
                break 'event_loop; // 超时，退出循环
            };

            // 检查事件是否与我们的进程相关
            if !event.is_valid() {
                continue;
            }

            // 检查进程状态
            let state = process.state();

            match state {
                lldb::StateType::Stopped => {
                    println!("Process STOPPED.");

                    // 遍历所有线程，看为什么停止
                    for thread in process.threads() {
                        let reason = thread.stop_reason();

                        if reason == StopReason::Breakpoint {
                            // 断点命中了！
                            println!("   Thread #{} >> HIT BREAKPOINT! <<", thread.index_id());
                            thread.set_selected_frame(0);
                            let frame = thread.selected_frame();
                            let args: Vec<_> = frame.arguments().iter().map(|x1| {
                                x1.value().unwrap_or_default().to_string()
                            }).collect();
                            println!("{:?}", args);
                            let x2 = debugger.execute_command("register read x2").unwrap();
                            let x2 = u64::from_str_radix(&x2[
                                x2.find("0x").unwrap() + 2..
                                    x2.find("0x").unwrap() + 18
                                ], 16).unwrap();
                            let mut buffer = [0u8; 16];
                            let mem = process.read_memory(
                                x2,
                                &mut buffer
                            );
                            println!("x2: {:x}", x2);
                            println!("read: {}", serde_json::to_string(&serde_json::Value::String(String::from_utf8_lossy(&buffer).into())).unwrap());

                            let result = debugger.execute_command("image list -o -f | grep /Applications/QQ.app/Contents/Resources/app/wrapper.node");
                            match result {
                                Ok(str) => {
                                    if str.contains("wrapper.node") {
                                        let num = &str[
                                            str.find("0x").unwrap() + 2..
                                                str.rfind(" /Application").unwrap()
                                            ];
                                        println!("num {}", num);
                                    }
                                }
                                _=>{}
                            }

                            println!("Continuing process...");
                            process.continue_execution().unwrap();
                        }
                    }
                }
                lldb::StateType::Exited => {
                    let exit_code = process.exit_status();
                    println!("Process EXITED with code: {}", exit_code);
                    break 'event_loop;
                }
                lldb::StateType::Crashed | lldb::StateType::Suspended => {
                    println!("Process Crashed or Suspended. Stopping.");
                    break 'event_loop;
                }
                _ => {
                    // 比如 Running, Stepping 等，我们暂时忽略
                }
            }
        }

        let i = index as u32;
        println!("{:?}", memmem::find(&mmap[..], &i.to_le_bytes()));
    } else {
        println!("Pattern not found.");
    }

    let mut matches = cmd().get_matches();
    let app: Box<dyn app::App> = match matches.remove_subcommand() {
        Some((s, matches)) if s == "export" => Box::new(app::export(matches)?),
        Some((s, matches)) if s == "serve" => Box::new(app::serve(matches)?),
        _ => Box::new(app::export(subcommand_export().get_matches())?),
    };
    app.run()?;
    Ok(())
}

fn common_args() -> [Arg; 4] {
    [
        arg!([file] "NT QQ 数据库文件。如果未提供，将尝试自动检测"),
        arg!(-p --pkey <pkey> "数据库密钥。如果未提供，将尝试自动探测"),
        arg!(-N --nocopy "默认情况下，会先将db文件复制到一个临时文件，再去操作临时文件。启用此选项以直接读取原始数据库文件。注意：这可能损坏你的数据库！")
        .action(ArgAction::SetTrue),
        arg!(--"android-uid" <UID> "如果确信这是一个 android NTQQ 的数据库，那么提供 uid 可以直接解密")
    ]
}
fn subcommand_export() -> Command {
    command!("export")
        .about("导出为未加密 sqlite 数据库")
        .args(common_args())
        .args([arg!(-o --output <PATH> "输出文件")
            .value_parser(value_parser!(PathBuf))
            .default_value("./nt_unwraped.db")])
}
fn cmd() -> Command {
    command!()
        .about("一键解密/解析 NTQQ 数据库！")
        .after_help(
            "可以不带任何subcommand运行此程序，默认进入 export 模式，并尝试自动探测所有参数。",
        )
        .args_conflicts_with_subcommands(true)
        .subcommand(subcommand_export())
        .subcommand(
            command!("hook")
                .about("尝试 hook 运行中的 NTQQ 进程以导出数据库文件。")
                .args(common_args())
        )
        .subcommand(
            command!("serve")
                .about("启动一个 web 服务，以通过 HTTP API 读取数据库内容。")
                .args(common_args())
                .args([arg!(-l --listen [listen] "监听地址")
                    .value_parser(value_parser!(std::net::SocketAddr))
                    .default_value("127.0.0.1:19551")]),
        )
}
