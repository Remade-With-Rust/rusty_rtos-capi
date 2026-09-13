/*
 * The board drivers' signatures, audited by the compiler.
 *
 * `seam/board.rs` defines eight functions that four demo files call:
 * `ParTest.c`'s three and `serial.c`'s four, plus the initialiser. They are
 * NOT FreeRTOS APIs, so they are deliberately absent from `symbols.rs` and
 * from the generated `kairos_capi.h` -- a demo *project* supplies them for
 * its board, and claiming the ABI exports them would be a lie about what
 * FreeRTOS is.
 *
 * That leaves them unaudited, which is the hole this file fills. The real
 * ABI symbols get checked because `header_gate.c` compiles the generated
 * header beside the oracle's own in ONE translation unit, so a disagreement
 * is "conflicting types". These would otherwise be checked by nothing: a
 * `UBaseType_t` written as `u32` in Rust against the C's 64-bit typedef
 * passes the linker and hands the callee half a register -- which is
 * exactly the defect that created `MuHigh` at priority 0 and cost an
 * afternoon. So include the oracle's own declarations and take the address
 * of each definition.
 *
 * `volatile`, for the reason the header gate records: without it the
 * compiler folds the table away, the linker discards it, and the check
 * becomes one that cannot fail.
 */
#include "FreeRTOS.h"
#include "task.h"

#include "partest.h"
#include "serial.h"

static void * volatile kairos_board_table[] =
{
    ( void * ) vParTestInitialise,
    ( void * ) vParTestSetLED,
    ( void * ) vParTestToggleLED,
    ( void * ) xSerialPortInitMinimal,
    ( void * ) xSerialPutChar,
    ( void * ) xSerialGetChar,
    ( void * ) vSerialPutString,
};

/* How many board symbols resolved, so a cell can print it beside the
 * header gate's count and a zero is visible rather than silent. */
unsigned long kairos_capi_board_gate( void )
{
    unsigned long resolved = 0;
    size_t i;

    for( i = 0; i < sizeof( kairos_board_table ) / sizeof( kairos_board_table[ 0 ] ); i++ )
    {
        if( kairos_board_table[ i ] != NULL )
        {
            resolved++;
        }
    }

    return resolved;
}
