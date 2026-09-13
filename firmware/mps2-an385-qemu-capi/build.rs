//! Compile the **unmodified** C demo task and hand it to the linker.
//!
//! Nothing here patches, wraps or regenerates the demo source. It is
//! compiled straight out of the pinned `oracle/` checkout against the real
//! `FreeRTOS.h`, `task.h` and `queue.h` from the same checkout — which is
//! the whole claim K6 makes. The only file this cell supplies to the C side
//! is `capi/FreeRTOSConfig.h`, which every FreeRTOS application supplies.
//!
//! If this ever needs a `#define` to paper over a difference, that is a
//! finding and belongs in the ledger, not in a compiler flag.

use std::path::PathBuf;

/// Walk up to the umbrella root, which is where `oracle/` lives.
fn umbrella_root() -> PathBuf {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"));
    // .../rusty_rtos-capi/firmware/mps2-an385-qemu-capi -> up three
    manifest
        .ancestors()
        .nth(3)
        .expect("the cell is three deep under the umbrella")
        .to_path_buf()
}

/// Write the generated header and its gate table into `OUT_DIR`.
///
/// The header is not checked in. A generated file in the tree is a file
/// somebody edits, and the next person cannot tell whether the edit or the
/// generator is authoritative -- so it is produced at build time from
/// `rusty_rtos-capi-core`'s symbol table, which is the same table the seam
/// is written against.
fn generate_header(out_dir: &std::path::Path) -> PathBuf {
    use std::fmt::Write as _;

    let mut header = String::new();
    rusty_rtos_capi_core::symbols::write_header(&mut header).expect("a String never fails");
    let header_path = out_dir.join(rusty_rtos_capi_core::symbols::HEADER_NAME);
    std::fs::write(&header_path, &header).expect("write the generated header");

    let mut gate = String::new();
    writeln!(gate).expect("a String never fails");
    rusty_rtos_capi_core::symbols::write_gate(&mut gate).expect("a String never fails");
    std::fs::write(out_dir.join("kairos_capi_gate.inc"), &gate).expect("write the gate table");

    header_path
}

fn main() {
    let root = umbrella_root();
    let kernel = root.join("oracle/FreeRTOS-Kernel");
    let demo = root.join("oracle/FreeRTOS/FreeRTOS/Demo");
    // The demo files this cell links, unmodified. Growing this list is how
    // K6 advances: each name added is one more of the thirty-four running
    // on the Kairos kernel instead of on FreeRTOS's own.
    const DEMOS: &[&str] = &[
        "AbortDelay",
        "BlockQ",
        "EventGroupsDemo",
        "GenQTest",
        "IntSemTest",
        "MessageBufferAMP",
        "MessageBufferDemo",
        "PollQ",
        "QPeek",
        "QueueOverwrite",
        "QueueSet",
        "QueueSetPolling",
        "StaticAllocation",
        "StreamBufferDemo",
        "StreamBufferInterrupt",
        "TaskNotify",
        "TaskNotifyArray",
        "TimerDemo",
        "blocktim",
        "comtest",
        "comtest_strings",
        "countsem",
        "crflash",
        "crhook",
        "death",
        "dynamic",
        "flash",
        "flash_timer",
        "flop",
        "integer",
        "recmutex",
        "semtest",
        "sp_flop",
    ];

    let minimal = demo.join("Common/Minimal");
    if !minimal.join("PollQ.c").is_file() {
        panic!(
            "no oracle checkout at {} -- run `kairos fetch` first",
            minimal.display()
        );
    }

    println!("cargo:rerun-if-changed=capi/FreeRTOSConfig.h");
    println!("cargo:rerun-if-changed=build.rs");
    for d in DEMOS {
        println!(
            "cargo:rerun-if-changed={}",
            minimal.join(format!("{d}.c")).display()
        );
    }

    let mut build = cc::Build::new();
    // clang, because this box has no `arm-none-eabi-gcc` and clang targets
    // thumbv7m directly. `cc` would otherwise go looking for a gcc triple.
    build
        .compiler("clang")
        // `cc` looks for `ar` by the target triple and finds none: this box
        // has no ARM binutils. LLVM's archiver is target-agnostic and is
        // already here, shipped beside the clang that does the compiling.
        .archiver("llvm-ar")
        .flag("-target")
        .flag("thumbv7m-none-eabi")
        .flag("-mcpu=cortex-m3")
        .flag("-ffreestanding")
        // The demo file includes <stdlib.h>; this box has no bare-metal ARM
        // sysroot, and the declarations are all it wants.
        .include(root.join("bench/kernel-ram/c/stub"))
        .include("capi")
        .include(kernel.join("include"))
        .include(kernel.join("portable/GCC/ARM_CM3"))
        .include(demo.join("Common/include"))
        .opt_level_str("s")
        .warnings(false);

    for d in DEMOS {
        build.file(minimal.join(format!("{d}.c")));
    }

    // The header gate. It compiles the oracle's real headers and the
    // generated one in the SAME translation unit, so any disagreement
    // between them is a compile error here rather than a surprise in
    // somebody else's project; and it takes the address of every declared
    // symbol, so a declaration the seam does not define is an undefined
    // reference at link time.
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("OUT_DIR"));
    let header = generate_header(&out_dir);
    println!("cargo:rerun-if-changed=capi/header_gate.c");
    // The board drivers' own gate. They are not ABI symbols, so the
    // generated header does not declare them and nothing else would check
    // their widths against the oracle's `partest.h` / `serial.h`.
    println!("cargo:rerun-if-changed=../../../seam/board_gate.c");
    println!("cargo:warning=generated {}", header.display());
    build
        .include(&out_dir)
        .file("capi/header_gate.c")
        .file(root.join("rusty_rtos-capi/seam/board_gate.c"));

    build.compile("freertos_demo");
}
