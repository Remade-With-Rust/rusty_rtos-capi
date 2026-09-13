#!/usr/bin/env python3
"""Re-derive the ABI symbol table from the seam.

    python tools/derive_symbols.py [--check]

The table in `crates/rusty_rtos-capi-core/src/symbols.rs` is what the
generated header comes from, and this is what the table comes from: the
`#[no_mangle] pub extern "C" fn` items in `seam/abi.rs`, the one
seam every C ABI cell compiles. Deriving it
rather than maintaining it by hand is the same discipline the SURFACE was
chosen by -- `llvm-nm -u` was asked what the demo files wanted, and nobody
got a vote.

`--check` re-derives and reports whether the table on disk is what would be
produced, without writing. That is the CI shape: a seam that grew a symbol
without the table growing one is a header that is now short by one.

The only judgement in here is the type mapping, and it is judgement the
compiler audits: `firmware/mps2-an385-qemu-capi/capi/header_gate.c`
compiles the generated header beside the oracle's real ones, so a type that
is mapped wrongly is a compile error rather than an opinion. Every entry in
PARAM, FUNC_PARAM and RET below was either right first time or corrected by
that gate.
"""
import argparse
import io
import os
import re
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
PKG = os.path.dirname(HERE)
SEAM = os.path.join(PKG, 'seam', 'abi.rs')
TABLE = os.path.join(PKG, 'crates', 'rusty_rtos-capi-core', 'src', 'symbols.rs')

# Rust type -> C type, for the scalars and the raw pointers.
SCALAR = {
    'BaseType_t': 'BaseType_t',
    'UBaseType_t': 'UBaseType_t',
    'TickType_t': 'TickType_t',
    'StackDepth_t': 'configSTACK_DEPTH_TYPE',
    'u32': 'uint32_t',
    'u16': 'uint16_t',
    'u8': 'uint8_t',
    'u64': 'uint64_t',
    'i32': 'BaseType_t',
    'usize': 'size_t',
    'bool': 'BaseType_t',
    '()': 'void',
    '*mut c_void': 'void *',
    '*const c_void': 'const void *',
    '*mut *mut c_void': 'void **',
    '*const core::ffi::c_char': 'const char *',
    '*mut BaseType_t': 'BaseType_t *',
    '*mut TickType_t': 'TickType_t *',
    '*mut u32': 'uint32_t *',
    '*const u32': 'const uint32_t *',
    '*mut usize': 'size_t *',
    'TaskFunction_t': 'TaskFunction_t',
}

# Parameter NAME -> the FreeRTOS typedef, where the C spells a handle.
#
# `void *` is the right ABI and the wrong declaration: FreeRTOS's handles
# are incomplete struct pointers, and a header that says `void *` cannot be
# included beside theirs.
PARAM = {
    'xQueue': 'QueueHandle_t',
    'xQueueOrSemaphore': 'QueueSetMemberHandle_t',
    'xQueueSet': 'QueueSetHandle_t',
    'xMutex': 'SemaphoreHandle_t',
    'xSemaphore': 'SemaphoreHandle_t',
    'xTask': 'TaskHandle_t',
    'xTaskToDelete': 'TaskHandle_t',
    'xTaskToNotify': 'TaskHandle_t',
    'xTaskToResume': 'TaskHandle_t',
    'xTaskToSuspend': 'TaskHandle_t',
    'xEventGroup': 'EventGroupHandle_t',
    'xStreamBuffer': 'StreamBufferHandle_t',
    'xTimer': 'TimerHandle_t',
    'pxCreatedTask': 'TaskHandle_t *',
    'pxTaskCode': 'TaskFunction_t',
    'pxSendCompletedCallback': 'StreamBufferCallbackFunction_t',
    'pxReceiveCompletedCallback': 'StreamBufferCallbackFunction_t',
}

# Where the parameter name alone is ambiguous: `pxCallbackFunction` is a
# `TimerCallbackFunction_t` to `xTimerCreate` and a `PendedFunction_t` to
# `xTimerPendFunctionCall`. Same spelling, two types.
FUNC_PARAM = {
    ('xTimerCreate', 'pxCallbackFunction'): 'TimerCallbackFunction_t',
    ('xTimerPendFunctionCall', 'pxCallbackFunction'): 'PendedFunction_t',
    ('xTimerPendFunctionCallFromISR', 'pxCallbackFunction'): 'PendedFunction_t',
}

# Function -> its return typedef, where it hands back a handle.
RET = {
    'xQueueGenericCreate': 'QueueHandle_t',
    'xQueueCreateCountingSemaphore': 'QueueHandle_t',
    'xQueueCreateMutex': 'QueueHandle_t',
    'xTaskGetHandle': 'TaskHandle_t',
    'xTaskGetCurrentTaskHandle': 'TaskHandle_t',
    'xQueueGetMutexHolder': 'TaskHandle_t',
    'xQueueGetMutexHolderFromISR': 'TaskHandle_t',
    'xQueueCreateSet': 'QueueSetHandle_t',
    'xQueueSelectFromSet': 'QueueSetMemberHandle_t',
    'xQueueSelectFromSetFromISR': 'QueueSetMemberHandle_t',
    'xEventGroupCreate': 'EventGroupHandle_t',
    'xStreamBufferGenericCreate': 'StreamBufferHandle_t',
    'xTimerCreate': 'TimerHandle_t',
}

DECL = re.compile(
    r'#\[no_mangle\]\s*\npub (?:unsafe )?extern "C" fn (\w+)\s*\((.*?)\)\s*(->\s*[^{]+)?\{',
    re.S,
)


def c_type(rust, where):
    t = ' '.join(rust.split())
    if t not in SCALAR:
        sys.exit('no C type for Rust `%s` (in %s) -- add it to SCALAR' % (t, where))
    return SCALAR[t]


def strip_comments(text):
    """Drop `//` comments.

    A parameter carrying an explanation above it is the normal case in this
    seam -- several of them record what the header gate found -- and a
    naive split on commas would take the comment for a type.
    """
    return re.sub(r'//[^\n]*', '', text)


# The seam's own section boundary, and everything below it is NOT the ABI.
#
# `strcmp`, `strncmp`, `strlen`, `sprintf` and `fabs` are there because a
# bare-metal cell has no libc to link -- not because FreeRTOS exports them.
# Declaring them in the generated header would claim an ABI surface that
# does not exist, and for `sprintf` it would CONFLICT with the real
# `<stdio.h>` in the header gate's own translation unit.
#
# A banner rather than a name list, so a libc function added below it is
# excluded without anyone remembering to, and one added above it is a
# mistake this tool does not silently absorb.
LIBC_BANNER = 'tiny libc =='


def derive():
    raw = io.open(SEAM, encoding='utf-8').read()
    # Find the banner BEFORE stripping comments, because the banner IS a
    # comment. Getting this the other way round made the tool report the
    # boundary missing from a file that has it.
    cut = raw.find(LIBC_BANNER)
    if cut < 0:
        sys.exit(
            'the seam no longer has a `%s` banner. It marks where the ABI '
            'ends and the no-libc stand-ins begin; without it this tool '
            'would declare `sprintf` as part of the ABI.' % LIBC_BANNER
        )
    seam = strip_comments(raw[:cut])
    rows = []
    for m in DECL.finditer(seam):
        name = m.group(1)
        raw_args = ' '.join(m.group(2).split())
        raw_ret = (m.group(3) or '').replace('->', '').strip()

        ret = RET.get(name, c_type(raw_ret, name) if raw_ret else 'void')

        args = []
        for part in [p for p in raw_args.split(',') if p.strip()]:
            pname, _, ptype = part.partition(':')
            # `_unused` in the Rust is still the C's parameter name.
            pname = pname.strip().lstrip('_')
            ctype = FUNC_PARAM.get((name, pname)) or PARAM.get(pname) \
                or c_type(ptype, '%s(%s)' % (name, pname))
            args.append(ctype + pname if ctype.endswith('*') else ctype + ' ' + pname)
        rows.append((name, ret, ', '.join(args) if args else 'void'))
    return rows


# One `Symbol { .. }` entry, however `rustfmt` chose to lay it out.
ENTRY = re.compile(
    r'Symbol\s*\{\s*name:\s*"([^"]*)"\s*,\s*ret:\s*"([^"]*)"\s*,'
    r'\s*args:\s*"([^"]*)"\s*,?\s*\}',
    re.S,
)


def parse(table):
    """The rows a table text describes, in order.

    Compared instead of the text itself because `rustfmt` owns the layout
    of a generated file and this tool owns its content. Comparing both got
    the two gates into a state where passing one meant failing the other.
    """
    return [(m.group(1), m.group(2), m.group(3)) for m in ENTRY.finditer(table)]


def render(rows):
    return '\n'.join(
        '    Symbol { name: "%s", ret: "%s", args: "%s" },' % r for r in rows
    )


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument('--check', action='store_true',
                    help='report drift instead of writing')
    opts = ap.parse_args()

    rows = derive()
    table = render(rows)

    src = io.open(TABLE, encoding='utf-8').read()
    start = src.index('pub const SYMBOLS: &[Symbol] = &[\n') + len(
        'pub const SYMBOLS: &[Symbol] = &[\n')
    end = src.index('\n];', start)

    if parse(src[start:end]) == rows:
        print('symbols.rs is up to date (%d symbols)' % len(rows))
        return 0
    if opts.check:
        print('DRIFT: the seam and the table disagree. Re-run without --check.')
        have = {r[0] for r in parse(src[start:end])}
        want = {r[0] for r in rows}
        for n in sorted(want - have):
            print('  seam has, table lacks: %s' % n)
        for n in sorted(have - want):
            print('  table has, seam lacks: %s' % n)
        return 1

    io.open(TABLE, 'w', encoding='utf-8').write(src[:start] + table + src[end:])
    print('rewrote symbols.rs (%d symbols)' % len(rows))
    return 0


if __name__ == '__main__':
    sys.exit(main())
