/*
 * The configuration the C demo task is compiled against.
 *
 * This is a CONSUMER's config, not the kernel's. Nothing here is read by
 * Kairos -- the Rust kernel's geometry is chosen in `src/main.rs` by the
 * `Config` impl and the arena constants. What this file does is satisfy the
 * real `FreeRTOS.h`, which the demo file includes unmodified, and give it
 * the handful of macros it expands: `configMINIMAL_STACK_SIZE`,
 * `configQUEUE_REGISTRY_SIZE`, `portTICK_PERIOD_MS`.
 *
 * The two halves have to AGREE, and where they must, it is written down:
 * `configTICK_RATE_HZ` below and `CapiConfig::TICK_RATE_HZ` in `main.rs`
 * are the same number, because `pdMS_TO_TICKS` in the C and `delay()` in
 * the Rust must mean the same duration.
 */
#ifndef FREERTOS_CONFIG_H
#define FREERTOS_CONFIG_H

#define configUSE_PREEMPTION                    1
#define configUSE_IDLE_HOOK                     0
#define configUSE_TICK_HOOK                     0
/* The board's clock, and it is a MEASURED number rather than a guess.
 *
 * QEMU's `mps2-an385` has no fixed clock: `configCPU_CLOCK_HZ` and the
 * SysTick reload in `main.rs` together decide how much emulated CPU a tick
 * gets, and the two must agree or the tick is not the rate the C thinks it
 * is. This number is therefore a BUDGET -- cycles of emulated CPU per
 * tick -- and the demo set has to fit inside it.
 *
 * It did not fit. Sixty tasks plus a tick hook that runs every demo's ISR
 * half EVERY tick, sharing 20,000 cycles, is about 330 cycles per task per
 * tick; the cell could not always finish a tick's work, and the
 * highest-priority task occasionally ran one tick late.
 * `StreamBufferDemo`'s `prvInterruptTriggerLevelTest` asks for a 5-tick
 * blocking receive while the tick hook adds a byte per tick, and asserts
 * on the exact count with ZERO margin -- so a single late tick fails it,
 * and its `xErrorStatus` latches for the rest of the run.
 *
 * Measured, all 21 demos, 30,000 ticks, counting receives that came back
 * one tick late:
 *
 *   20 MHz -> 2      40 MHz -> 2      80 MHz -> 0
 *
 * So it is not a rate: the budget has to cross the set's actual per-tick
 * cost, and between 40 and 80 MHz it does. At 80 MHz the byte histogram is
 * `[_, _, 58, 58, 58, 115, 0, 0, 0, 0]` -- identical to the host cell's,
 * which has real threads and no budget at all. That is the evidence this
 * is the emulator's throughput and not a kernel defect: the same kernel,
 * the same seam and the same demo, given enough CPU, produces the host's
 * histogram exactly.
 *
 * The cost is wall-clock: QEMU runs 4x the instructions per tick, so a
 * 30,000-tick sustained run takes correspondingly longer. Paid knowingly.
 *
 * `configTICK_RATE_HZ` is unchanged at 1000, so every demo's timing -- all
 * of it counted in TICKS -- is unchanged. What changes is only how much
 * work the emulated part can do between two of them. */
#define configCPU_CLOCK_HZ                      ( 80000000UL )
#define configTICK_RATE_HZ                      ( ( TickType_t ) 1000 )
#define configMAX_PRIORITIES                    ( 5 )
#define configMINIMAL_STACK_SIZE                ( ( unsigned short ) 256 )
#define configTOTAL_HEAP_SIZE                   ( ( size_t ) 0 )
#define configMAX_TASK_NAME_LEN                 ( 16 )
#define configIDLE_SHOULD_YIELD                 1
#define configUSE_MUTEXES                       1
#define configUSE_COUNTING_SEMAPHORES           1
#define configQUEUE_REGISTRY_SIZE                0
#define configUSE_TASK_NOTIFICATIONS            1
/* TaskNotifyArray.c refuses to build below 3, and the Rust `CapiConfig`
 * carries the same number: the two configs describe ONE system and a
 * disagreement here is an ABI bug that compiles cleanly on both sides. */
#define configTASK_NOTIFICATION_ARRAY_ENTRIES   3
#define configUSE_TIMERS                        1
#define configTIMER_TASK_PRIORITY               ( 2 )
#define configTIMER_QUEUE_LENGTH                ( 10 )
#define configTIMER_TASK_STACK_DEPTH            ( 256 )
#define configUSE_RECURSIVE_MUTEXES             1
#define configUSE_QUEUE_SETS                    1
#define configUSE_APPLICATION_TASK_TAG          0
#define configTICK_TYPE_WIDTH_IN_BITS           TICK_TYPE_WIDTH_32_BITS

/* ARM_CM3's `portmacro.h` builds its BASEPRI masking around this, so a
 * config that omits it does not compile. It is a PORT number and Kairos
 * does not read it -- our critical section is `CortexMPort`'s -- but the
 * oracle's header is the one the demo includes, so the oracle's
 * requirements are the ones that must be met. 5 is the usual demo value. */
#define configMAX_SYSCALL_INTERRUPT_PRIORITY    ( 5 << ( 8 - 3 ) )
#define configPRIO_BITS                         3
#define configKERNEL_INTERRUPT_PRIORITY         ( 7 << ( 8 - 3 ) )

/* No heap: Kairos allocates tasks and queues from arenas declared at
 * compile time, so there is nothing for the C side to allocate from and
 * nothing for it to allocate. `xQueueCreate` reaches our `xQueueGenericCreate`,
 * which asks the kernel's arena. */
#define configSUPPORT_DYNAMIC_ALLOCATION        1
#define configSUPPORT_STATIC_ALLOCATION         0

#define INCLUDE_vTaskPrioritySet                1
#define INCLUDE_uxTaskPriorityGet               1
#define INCLUDE_vTaskDelete                     1
#define INCLUDE_vTaskSuspend                    1
#define INCLUDE_xTaskDelayUntil                 1
#define INCLUDE_vTaskDelay                      1
#define INCLUDE_xTaskGetSchedulerState          1
#define INCLUDE_eTaskGetState                   1
#define INCLUDE_xTimerPendFunctionCall          1
#define INCLUDE_xTaskAbortDelay                 1
#define INCLUDE_xQueueGetMutexHolder            1
#define INCLUDE_xSemaphoreGetMutexHolder        1
#define INCLUDE_xTaskGetHandle                  1
#define INCLUDE_uxTaskGetStackHighWaterMark     0

/* `configASSERT` is deliberately LOUD rather than absent. A demo file that
 * trips one is telling us the ABI lied to it, and that is the single most
 * valuable signal this cell can produce -- far better than a wrong answer
 * arriving quietly. */
#include <stdint.h>
extern void vCapiAssertFailed( const char * file, uint32_t line );
#define configASSERT( x )    if( ( x ) == 0 ) { vCapiAssertFailed( __FILE__, __LINE__ ); }

#endif /* FREERTOS_CONFIG_H */
