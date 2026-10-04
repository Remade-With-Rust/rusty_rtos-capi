/*
 * The configuration the C demo task is compiled against, HOST cell.
 *
 * Deliberately the same file as `firmware/mps2-an385-qemu-capi/capi/`'s
 * apart from the interrupt-priority block below, which is a port fact. If
 * these two ever diverge in anything else, the two cells have stopped
 * being the same experiment on two ports.
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
#define configCPU_CLOCK_HZ                      ( 20000000UL )
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

/* The host port has no interrupt priorities to mask -- its "interrupt" is
 * another OS thread and the mask is a lock that thread agrees to take -- so
 * the three ARM_CM3 numbers that `portmacro.h` builds its BASEPRI masking
 * around are simply absent here. That is the whole difference between this
 * config and the Cortex-M3 one, which is worth saying out loud: the CELLS
 * differ by a port, not by an API. */

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
/* Two cores (`cargo run --features smp`, umbrella HOLES.md H15).
 *
 * The C here is the demo files only -- the kernel is Kairos -- so the SMP
 * port macros FreeRTOS.h demands are reached only through the task.h macros
 * the demos expand. The interrupt-mask pair keeps the meaning the one-core
 * build gives it; the core id and the cross-core yield name functions this
 * cell does not define, so a demo that ever expanded one would fail to LINK
 * rather than quietly read core 0. The task and ISR locks are the kernel's
 * business, and the kernel is not this C. */
#ifdef KAIROS_CAPI_SMP
    #define configNUMBER_OF_CORES                   2
    #define configUSE_PASSIVE_IDLE_HOOK             0
    #define configRUN_MULTIPLE_PRIORITIES           1
    #define configUSE_CORE_AFFINITY                 0
    #define configUSE_PORT_OPTIMISED_TASK_SELECTION 0
    /* StreamBufferDemo's trigger-level receive asserts its byte count with
     * a margin of 0 by default, and names this setting for a platform where
     * a woken task can be served a tick late. Two virtual cores on a host
     * are such a platform: the woken task's OS thread must be scheduled
     * again before its call can return, and under load that can cross the
     * one-millisecond wall-clock tick (measured: 17 to 22 receives a tick
     * late per 30,000 ticks, never two). One-core build: unchanged, 0. */
    #define configSTREAM_BUFFER_TRIGGER_LEVEL_TEST_MARGIN 1
    #define portGET_CORE_ID()                       xCapiNotACoreId()
    #define portYIELD_CORE( x )                     vCapiNotACoreYield( x )
    #define portSET_INTERRUPT_MASK()                0
    #define portCLEAR_INTERRUPT_MASK( x )           ( void ) ( x )
    #define portENTER_CRITICAL_FROM_ISR()           portSET_INTERRUPT_MASK_FROM_ISR()
    #define portEXIT_CRITICAL_FROM_ISR( x )         portCLEAR_INTERRUPT_MASK_FROM_ISR( x )
    #define portGET_TASK_LOCK( xCoreID )
    #define portRELEASE_TASK_LOCK( xCoreID )
    #define portGET_ISR_LOCK( xCoreID )
    #define portRELEASE_ISR_LOCK( xCoreID )
#endif

#include <stdint.h>
extern void vCapiAssertFailed( const char * file, uint32_t line );
#define configASSERT( x )    if( ( x ) == 0 ) { vCapiAssertFailed( __FILE__, __LINE__ ); }

#endif /* FREERTOS_CONFIG_H */
