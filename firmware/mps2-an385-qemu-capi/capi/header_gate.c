/*
 * The header gate.
 *
 * `kairos_capi.h` is GENERATED from `rusty_rtos-capi-core`'s symbol table,
 * and a generated header is worth exactly what is checked about it. Two
 * things are checked here, by the C compiler and the linker rather than by
 * anyone's reading:
 *
 * 1. **Compatibility.** This file includes the ORACLE's real FreeRTOS
 *    headers and then ours. A declaration of ours that disagrees with
 *    FreeRTOS's -- a handle spelled `void *` where they spell it
 *    `struct QueueDefinition *`, a parameter of the wrong width, a
 *    forgotten `const` that changes the type -- is "conflicting types for
 *    'xQueueSend'" and the build stops. That is the claim the crate makes
 *    ("a C program relinks") tested the only way it can be.
 *
 * 2. **Completeness.** Taking the address of every declared symbol into a
 *    table means a declaration with no definition is an UNDEFINED
 *    REFERENCE at link time. A header that promises a function the seam
 *    does not export would otherwise link cleanly right up until the call
 *    site was reached -- which, for an API this size, could be months.
 *
 * The table is `volatile` and `const`-initialised so nothing may fold it
 * away; `used` on the section keeps the linker from discarding it.
 */
#include "FreeRTOSConfig.h"
#include "FreeRTOS.h"
#include "task.h"
#include "queue.h"
#include "semphr.h"
#include "timers.h"
#include "event_groups.h"
#include "stream_buffer.h"
#include "message_buffer.h"

/* Ours, second and deliberately: it has to survive theirs. */
#include "kairos_capi.h"

#include "kairos_capi_gate.inc"

/*
 * How many of the declared symbols resolved to a real address.
 *
 * This must be CALLED, and `main.rs` calls it. Left unreferenced the
 * linker discards the section with `--gc-sections`, its relocations are
 * never resolved, and the completeness half of this gate silently checks
 * nothing at all -- which is what happened on the first attempt: the table
 * was built, the build was green, and `llvm-nm` found no trace of it in
 * the ELF. A gate nothing reaches is not a gate.
 */
unsigned long kairos_capi_header_gate( void )
{
    unsigned long i, resolved = 0;
    const unsigned long n =
        sizeof( kairos_capi_symbol_addresses ) / sizeof( kairos_capi_symbol_addresses[ 0 ] );

    for( i = 0; i < n; i++ )
    {
        if( kairos_capi_symbol_addresses[ i ] != 0 )
        {
            resolved++;
        }
    }

    return resolved;
}
