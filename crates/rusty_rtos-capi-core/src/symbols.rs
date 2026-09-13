//! The ABI as data, and the header generated from it.
//!
//! # The specification was derived, not chosen
//!
//! The thirty-three demo files were compiled unmodified against the
//! oracle's own headers and `llvm-nm -u` was asked what they wanted. That
//! list IS the requirement, and it is far smaller than the API map
//! suggests: 108 undefined symbols, of which eleven are `__aeabi_*`
//! compiler float helpers and six are board drivers, leaving a kernel
//! surface of about eighty-four.
//!
//! # Why the list is a table and not a header file
//!
//! A hand-written `.h` beside a hand-written seam is two lists that drift,
//! and the drift is silent in the direction that matters: a header
//! declaring a function the seam does not export links cleanly until the
//! call site is reached. So the header is GENERATED from this table.
//!
//! The gate is in `firmware/mps2-an385-qemu-capi`, whose `build.rs`
//! compiles a C translation unit that includes the generated header and
//! takes the address of every symbol it declares. A declaration with no
//! definition then fails to LINK, and a definition whose signature
//! disagrees fails to COMPILE.

use core::fmt::Write;

/// One `extern "C"` entry point: the C declaration, in three pieces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Symbol {
    /// The linker name, which is also the C name -- these are `#[no_mangle]`.
    pub name: &'static str,
    /// The C return type, `"void"` for none.
    pub ret: &'static str,
    /// The C parameter list, `"void"` for none.
    pub args: &'static str,
}

/// Every symbol this ABI exports.
///
/// Kept in the order the seam declares them, which is the order the demo
/// files needed them in: `PollQ.c`'s eight first, then each file's new
/// ones behind it. That ordering is the derivation's own record and is
/// worth more than alphabetical.
pub const SYMBOLS: &[Symbol] = &[
    Symbol {
        name: "xTaskCreate",
        ret: "BaseType_t",
        args: "TaskFunction_t pxTaskCode, const char *pcName, configSTACK_DEPTH_TYPE uxStackDepth, void *pvParameters, UBaseType_t uxPriority, TaskHandle_t *pxCreatedTask",
    },
    Symbol {
        name: "vTaskDelay",
        ret: "void",
        args: "TickType_t xTicksToDelay",
    },
    Symbol {
        name: "xQueueGenericCreate",
        ret: "QueueHandle_t",
        args: "UBaseType_t uxQueueLength, UBaseType_t uxItemSize, uint8_t ucQueueType",
    },
    Symbol {
        name: "xQueueGenericSend",
        ret: "BaseType_t",
        args: "QueueHandle_t xQueue, const void *pvItemToQueue, TickType_t xTicksToWait, BaseType_t xCopyPosition",
    },
    Symbol {
        name: "xQueueReceive",
        ret: "BaseType_t",
        args: "QueueHandle_t xQueue, void *pvBuffer, TickType_t xTicksToWait",
    },
    Symbol {
        name: "uxQueueMessagesWaiting",
        ret: "UBaseType_t",
        args: "QueueHandle_t xQueue",
    },
    Symbol {
        name: "xQueueSemaphoreTake",
        ret: "BaseType_t",
        args: "QueueHandle_t xQueue, TickType_t xTicksToWait",
    },
    Symbol {
        name: "xQueueCreateCountingSemaphore",
        ret: "QueueHandle_t",
        args: "UBaseType_t uxMaxCount, UBaseType_t uxInitialCount",
    },
    Symbol {
        name: "xQueueCreateMutex",
        ret: "QueueHandle_t",
        args: "uint8_t ucQueueType",
    },
    Symbol {
        name: "xTaskGetTickCount",
        ret: "TickType_t",
        args: "void",
    },
    Symbol {
        name: "xTaskDelayUntil",
        ret: "BaseType_t",
        args: "TickType_t *pxPreviousWakeTime, TickType_t xTimeIncrement",
    },
    Symbol {
        name: "vTaskSuspend",
        ret: "void",
        args: "TaskHandle_t xTaskToSuspend",
    },
    Symbol {
        name: "vTaskResume",
        ret: "void",
        args: "TaskHandle_t xTaskToResume",
    },
    Symbol {
        name: "xQueuePeek",
        ret: "BaseType_t",
        args: "QueueHandle_t xQueue, void *pvBuffer, TickType_t xTicksToWait",
    },
    Symbol {
        name: "pvPortMalloc",
        ret: "void *",
        args: "size_t xWantedSize",
    },
    Symbol {
        name: "vPortFree",
        ret: "void",
        args: "void *pv",
    },
    Symbol {
        name: "xPortGetFreeHeapSize",
        ret: "size_t",
        args: "void",
    },
    Symbol {
        name: "vPortEnterCritical",
        ret: "void",
        args: "void",
    },
    Symbol {
        name: "vPortExitCritical",
        ret: "void",
        args: "void",
    },
    Symbol {
        name: "vPortGenerateSimulatedInterrupt",
        ret: "void",
        args: "uint32_t ulInterruptNumber",
    },
    Symbol {
        name: "vCapiAssertFailed",
        ret: "void",
        args: "const char *file, uint32_t line",
    },
    Symbol {
        name: "vTaskDelete",
        ret: "void",
        args: "TaskHandle_t xTaskToDelete",
    },
    Symbol {
        name: "vTaskPrioritySet",
        ret: "void",
        args: "TaskHandle_t xTask, UBaseType_t uxNewPriority",
    },
    Symbol {
        name: "uxTaskPriorityGet",
        ret: "UBaseType_t",
        args: "TaskHandle_t xTask",
    },
    Symbol {
        name: "eTaskGetState",
        ret: "uint32_t",
        args: "TaskHandle_t xTask",
    },
    Symbol {
        name: "xTaskGetHandle",
        ret: "TaskHandle_t",
        args: "const char *pcNameToQuery",
    },
    Symbol {
        name: "xTaskGetCurrentTaskHandle",
        ret: "TaskHandle_t",
        args: "void",
    },
    Symbol {
        name: "xTaskAbortDelay",
        ret: "BaseType_t",
        args: "TaskHandle_t xTask",
    },
    Symbol {
        name: "uxTaskGetNumberOfTasks",
        ret: "UBaseType_t",
        args: "void",
    },
    Symbol {
        name: "vTaskSuspendAll",
        ret: "void",
        args: "void",
    },
    Symbol {
        name: "xTaskResumeAll",
        ret: "BaseType_t",
        args: "void",
    },
    Symbol {
        name: "xTaskCatchUpTicks",
        ret: "BaseType_t",
        args: "TickType_t xTicksToCatchUp",
    },
    Symbol {
        name: "xTaskGetTickCountFromISR",
        ret: "TickType_t",
        args: "void",
    },
    Symbol {
        name: "xTaskGenericNotify",
        ret: "BaseType_t",
        args: "TaskHandle_t xTaskToNotify, UBaseType_t uxIndexToNotify, uint32_t ulValue, uint32_t eAction, uint32_t *pulPreviousNotificationValue",
    },
    Symbol {
        name: "xTaskGenericNotifyWait",
        ret: "BaseType_t",
        args: "UBaseType_t uxIndexToWaitOn, uint32_t ulBitsToClearOnEntry, uint32_t ulBitsToClearOnExit, uint32_t *pulNotificationValue, TickType_t xTicksToWait",
    },
    Symbol {
        name: "ulTaskGenericNotifyTake",
        ret: "uint32_t",
        args: "UBaseType_t uxIndexToWaitOn, BaseType_t xClearCountOnExit, TickType_t xTicksToWait",
    },
    Symbol {
        name: "xTaskGenericNotifyStateClear",
        ret: "BaseType_t",
        args: "TaskHandle_t xTask, UBaseType_t uxIndexToClear",
    },
    Symbol {
        name: "ulTaskGenericNotifyValueClear",
        ret: "uint32_t",
        args: "TaskHandle_t xTask, UBaseType_t uxIndexToClear, uint32_t ulBitsToClear",
    },
    Symbol {
        name: "xTaskGenericNotifyFromISR",
        ret: "BaseType_t",
        args: "TaskHandle_t xTaskToNotify, UBaseType_t uxIndexToNotify, uint32_t ulValue, uint32_t eAction, uint32_t *pulPreviousNotificationValue, BaseType_t *pxHigherPriorityTaskWoken",
    },
    Symbol {
        name: "vTaskGenericNotifyGiveFromISR",
        ret: "void",
        args: "TaskHandle_t xTaskToNotify, UBaseType_t uxIndexToNotify, BaseType_t *pxHigherPriorityTaskWoken",
    },
    Symbol {
        name: "vQueueDelete",
        ret: "void",
        args: "QueueHandle_t xQueue",
    },
    Symbol {
        name: "uxQueueSpacesAvailable",
        ret: "UBaseType_t",
        args: "QueueHandle_t xQueue",
    },
    Symbol {
        name: "xQueueGenericReset",
        ret: "BaseType_t",
        args: "QueueHandle_t xQueue, BaseType_t xNewQueue",
    },
    Symbol {
        name: "xQueueGenericSendFromISR",
        ret: "BaseType_t",
        args: "QueueHandle_t xQueue, const void *pvItemToQueue, BaseType_t *pxHigherPriorityTaskWoken, BaseType_t xCopyPosition",
    },
    Symbol {
        name: "xQueueReceiveFromISR",
        ret: "BaseType_t",
        args: "QueueHandle_t xQueue, void *pvBuffer, BaseType_t *pxHigherPriorityTaskWoken",
    },
    Symbol {
        name: "xQueuePeekFromISR",
        ret: "BaseType_t",
        args: "QueueHandle_t xQueue, void *pvBuffer",
    },
    Symbol {
        name: "xQueueGiveFromISR",
        ret: "BaseType_t",
        args: "QueueHandle_t xQueue, BaseType_t *pxHigherPriorityTaskWoken",
    },
    Symbol {
        name: "xQueueGetMutexHolder",
        ret: "TaskHandle_t",
        args: "SemaphoreHandle_t xSemaphore",
    },
    Symbol {
        name: "xQueueGetMutexHolderFromISR",
        ret: "TaskHandle_t",
        args: "SemaphoreHandle_t xSemaphore",
    },
    Symbol {
        name: "xQueueGiveMutexRecursive",
        ret: "BaseType_t",
        args: "SemaphoreHandle_t xMutex",
    },
    Symbol {
        name: "xQueueTakeMutexRecursive",
        ret: "BaseType_t",
        args: "SemaphoreHandle_t xMutex, TickType_t xTicksToWait",
    },
    Symbol {
        name: "xQueueCreateSet",
        ret: "QueueSetHandle_t",
        args: "UBaseType_t uxEventQueueLength",
    },
    Symbol {
        name: "xQueueAddToSet",
        ret: "BaseType_t",
        args: "QueueSetMemberHandle_t xQueueOrSemaphore, QueueSetHandle_t xQueueSet",
    },
    Symbol {
        name: "xQueueRemoveFromSet",
        ret: "BaseType_t",
        args: "QueueSetMemberHandle_t xQueueOrSemaphore, QueueSetHandle_t xQueueSet",
    },
    Symbol {
        name: "xQueueSelectFromSet",
        ret: "QueueSetMemberHandle_t",
        args: "QueueSetHandle_t xQueueSet, TickType_t xTicksToWait",
    },
    Symbol {
        name: "xQueueSelectFromSetFromISR",
        ret: "QueueSetMemberHandle_t",
        args: "QueueSetHandle_t xQueueSet",
    },
    Symbol {
        name: "xEventGroupCreate",
        ret: "EventGroupHandle_t",
        args: "void",
    },
    Symbol {
        name: "vEventGroupDelete",
        ret: "void",
        args: "EventGroupHandle_t xEventGroup",
    },
    Symbol {
        name: "xEventGroupSetBits",
        ret: "uint32_t",
        args: "EventGroupHandle_t xEventGroup, uint32_t uxBitsToSet",
    },
    Symbol {
        name: "xEventGroupClearBits",
        ret: "uint32_t",
        args: "EventGroupHandle_t xEventGroup, uint32_t uxBitsToClear",
    },
    Symbol {
        name: "xEventGroupWaitBits",
        ret: "uint32_t",
        args: "EventGroupHandle_t xEventGroup, uint32_t uxBitsToWaitFor, BaseType_t xClearOnExit, BaseType_t xWaitForAllBits, TickType_t xTicksToWait",
    },
    Symbol {
        name: "xEventGroupSync",
        ret: "uint32_t",
        args: "EventGroupHandle_t xEventGroup, uint32_t uxBitsToSet, uint32_t uxBitsToWaitFor, TickType_t xTicksToWait",
    },
    Symbol {
        name: "xEventGroupGetBitsFromISR",
        ret: "uint32_t",
        args: "EventGroupHandle_t xEventGroup",
    },
    Symbol {
        name: "xEventGroupSetBitsFromISR",
        ret: "BaseType_t",
        args: "EventGroupHandle_t xEventGroup, uint32_t uxBitsToSet, BaseType_t *pxHigherPriorityTaskWoken",
    },
    Symbol {
        name: "xEventGroupClearBitsFromISR",
        ret: "BaseType_t",
        args: "EventGroupHandle_t xEventGroup, uint32_t uxBitsToClear",
    },
    Symbol {
        name: "xStreamBufferGenericCreate",
        ret: "StreamBufferHandle_t",
        args: "size_t xBufferSizeBytes, size_t xTriggerLevelBytes, BaseType_t xIsMessageBuffer, StreamBufferCallbackFunction_t pxSendCompletedCallback, StreamBufferCallbackFunction_t pxReceiveCompletedCallback",
    },
    Symbol {
        name: "vStreamBufferDelete",
        ret: "void",
        args: "StreamBufferHandle_t xStreamBuffer",
    },
    Symbol {
        name: "xStreamBufferSend",
        ret: "size_t",
        args: "StreamBufferHandle_t xStreamBuffer, const void *pvTxData, size_t xDataLengthBytes, TickType_t xTicksToWait",
    },
    Symbol {
        name: "xStreamBufferReceive",
        ret: "size_t",
        args: "StreamBufferHandle_t xStreamBuffer, void *pvRxData, size_t xBufferLengthBytes, TickType_t xTicksToWait",
    },
    Symbol {
        name: "xStreamBufferSendFromISR",
        ret: "size_t",
        args: "StreamBufferHandle_t xStreamBuffer, const void *pvTxData, size_t xDataLengthBytes, BaseType_t *pxHigherPriorityTaskWoken",
    },
    Symbol {
        name: "xStreamBufferReceiveFromISR",
        ret: "size_t",
        args: "StreamBufferHandle_t xStreamBuffer, void *pvRxData, size_t xBufferLengthBytes, BaseType_t *pxHigherPriorityTaskWoken",
    },
    Symbol {
        name: "xStreamBufferBytesAvailable",
        ret: "size_t",
        args: "StreamBufferHandle_t xStreamBuffer",
    },
    Symbol {
        name: "xStreamBufferSpacesAvailable",
        ret: "size_t",
        args: "StreamBufferHandle_t xStreamBuffer",
    },
    Symbol {
        name: "xStreamBufferIsEmpty",
        ret: "BaseType_t",
        args: "StreamBufferHandle_t xStreamBuffer",
    },
    Symbol {
        name: "xStreamBufferIsFull",
        ret: "BaseType_t",
        args: "StreamBufferHandle_t xStreamBuffer",
    },
    Symbol {
        name: "xStreamBufferReset",
        ret: "BaseType_t",
        args: "StreamBufferHandle_t xStreamBuffer",
    },
    Symbol {
        name: "xStreamBufferNextMessageLengthBytes",
        ret: "size_t",
        args: "StreamBufferHandle_t xStreamBuffer",
    },
    Symbol {
        name: "xStreamBufferSendCompletedFromISR",
        ret: "BaseType_t",
        args: "StreamBufferHandle_t xStreamBuffer, BaseType_t *pxHigherPriorityTaskWoken",
    },
    Symbol {
        name: "xTimerCreate",
        ret: "TimerHandle_t",
        args: "const char *pcTimerName, TickType_t xTimerPeriodInTicks, BaseType_t xAutoReload, void *pvTimerID, TimerCallbackFunction_t pxCallbackFunction",
    },
    Symbol {
        name: "xTimerGenericCommandFromTask",
        ret: "BaseType_t",
        args: "TimerHandle_t xTimer, BaseType_t xCommandID, TickType_t xOptionalValue, BaseType_t *pxHigherPriorityTaskWoken, TickType_t xTicksToWait",
    },
    Symbol {
        name: "xTimerGenericCommandFromISR",
        ret: "BaseType_t",
        args: "TimerHandle_t xTimer, BaseType_t xCommandID, TickType_t xOptionalValue, BaseType_t *pxHigherPriorityTaskWoken, TickType_t xTicksToWait",
    },
    Symbol {
        name: "xTimerIsTimerActive",
        ret: "BaseType_t",
        args: "TimerHandle_t xTimer",
    },
    Symbol {
        name: "pvTimerGetTimerID",
        ret: "void *",
        args: "TimerHandle_t xTimer",
    },
    Symbol {
        name: "vTimerSetTimerID",
        ret: "void",
        args: "TimerHandle_t xTimer, void *pvNewID",
    },
    Symbol {
        name: "pcTimerGetName",
        ret: "const char *",
        args: "TimerHandle_t xTimer",
    },
    Symbol {
        name: "uxTimerGetReloadMode",
        ret: "UBaseType_t",
        args: "TimerHandle_t xTimer",
    },
    Symbol {
        name: "vTimerSetReloadMode",
        ret: "void",
        args: "TimerHandle_t xTimer, BaseType_t xAutoReload",
    },
];

/// The name of the generated header.
pub const HEADER_NAME: &str = "kairos_capi.h";

/// Write the generated header.
///
/// It is deliberately NOT a replacement for `FreeRTOS.h`: it declares what
/// this ABI exports and typedefs the handle types, so a C program that
/// already has a `FreeRTOSConfig.h` can include it and link. The oracle's
/// own headers stay the ones the K6 cell compiles the demos against,
/// because compiling them against OUR header would prove only that we
/// agree with ourselves.
///
/// # Errors
/// Whatever the sink returns.
pub fn write_header(out: &mut impl Write) -> core::fmt::Result {
    writeln!(
        out,
        "/* {HEADER_NAME} -- GENERATED from rusty_rtos-capi-core."
    )?;
    writeln!(out, " *")?;
    writeln!(
        out,
        " * Do not edit. It comes from `symbols::SYMBOLS`, and the seam"
    )?;
    writeln!(out, " * is written against that same table.")?;
    writeln!(out, " *")?;
    writeln!(out, " * {} symbols.", SYMBOLS.len())?;
    writeln!(out, " */")?;
    writeln!(out, "#ifndef KAIROS_CAPI_H")?;
    writeln!(out, "#define KAIROS_CAPI_H")?;
    writeln!(out)?;
    writeln!(out, "#include <stdint.h>")?;
    writeln!(out, "#include <stddef.h>")?;
    writeln!(out)?;
    writeln!(out, "#ifdef __cplusplus")?;
    writeln!(out, "extern \"C\" {{")?;
    writeln!(out, "#endif")?;
    writeln!(out)?;
    write_types(out)?;
    writeln!(out)?;
    for s in SYMBOLS {
        writeln!(out, "{} {}( {} );", s.ret, s.name, s.args)?;
    }
    writeln!(out)?;
    writeln!(out, "#ifdef __cplusplus")?;
    writeln!(out, "}}")?;
    writeln!(out, "#endif")?;
    writeln!(out)?;
    writeln!(out, "#endif /* KAIROS_CAPI_H */")
}

/// Write the gate's address table: one entry per declared symbol.
///
/// Taking the address of a function DECLARED but not DEFINED is an
/// undefined reference at link time, which is the only way to prove a
/// generated header does not promise more than the seam delivers. The
/// alternative -- reading the two lists side by side -- is how they drift.
///
/// # Errors
/// Whatever the sink returns.
pub fn write_gate(out: &mut impl Write) -> core::fmt::Result {
    writeln!(
        out,
        "/* GENERATED. One address per declared symbol; see header_gate.c. */"
    )?;
    writeln!(out, "/*")?;
    writeln!(
        out,
        " * `volatile` is load-bearing, not decoration. Without it the"
    )?;
    writeln!(
        out,
        " * compiler proves every `&fn` is non-null, folds the whole walk"
    )?;
    writeln!(
        out,
        " * to a constant, and emits NO RELOCATIONS -- at which point the"
    )?;
    writeln!(
        out,
        " * linker has nothing to resolve and an undeclared-but-undefined"
    )?;
    writeln!(
        out,
        " * symbol sails through. Measured: the gate function compiled to"
    )?;
    writeln!(
        out,
        " * four bytes and the table was absent from the ELF entirely."
    )?;
    writeln!(out, " */")?;
    writeln!(
        out,
        "const void * const volatile kairos_capi_symbol_addresses[ {} ] = {{",
        SYMBOLS.len()
    )?;
    for s in SYMBOLS {
        writeln!(out, "    ( const void * ) &{},", s.name)?;
    }
    writeln!(out, "}};")
}

/// The typedefs and constants the declarations are written in.
///
/// # Every group is guarded by the FreeRTOS header that owns it
///
/// The normal case for this header is a C program that ALREADY has
/// FreeRTOS's headers and its own `FreeRTOSConfig.h`, and adds ours to
/// relink against the Kairos kernel. In that case FreeRTOS's typedefs are
/// the ones in force and ours must stand aside -- a second
/// `typedef void * QueueHandle_t` beside their
/// `typedef struct QueueDefinition * QueueHandle_t` is a hard error, and
/// every declaration below it becomes "conflicting types".
///
/// So each group is guarded by the include guard of the header that
/// defines it, and each typedef is the SAME SHAPE as FreeRTOS's --
/// an incomplete struct pointer, not `void *`. The two are the same to the
/// linker and different to the compiler, and the compiler is the one that
/// has to accept both headers at once.
///
/// `firmware/mps2-an385-qemu-capi/capi/header_gate.c` includes both and is
/// where that is checked.
fn write_types(out: &mut impl Write) -> core::fmt::Result {
    writeln!(
        out,
        "/* `portBASE_TYPE` is a PORT fact: `long` on ARM_CM3, `long long`"
    )?;
    writeln!(
        out,
        " * under _WIN64 and on 64-bit Unix, exactly as `portmacro.h` for"
    )?;
    writeln!(
        out,
        " * those ports decides it. The same question, asked the same way,"
    )?;
    writeln!(
        out,
        " * so a C program and `ctypes.rs` cannot reach different answers. */"
    )?;
    writeln!(out, "#ifndef INC_FREERTOS_H")?;
    writeln!(
        out,
        "    #if defined( _WIN64 ) || defined( __LP64__ ) || defined( _LP64 )"
    )?;
    writeln!(out, "typedef long long          BaseType_t;")?;
    writeln!(out, "typedef unsigned long long UBaseType_t;")?;
    writeln!(out, "typedef size_t             StackType_t;")?;
    writeln!(out, "    #else")?;
    writeln!(out, "typedef long          BaseType_t;")?;
    writeln!(out, "typedef unsigned long UBaseType_t;")?;
    writeln!(out, "typedef uint32_t      StackType_t;")?;
    writeln!(out, "    #endif")?;
    writeln!(out, "typedef uint32_t      TickType_t;")?;
    writeln!(out, "#endif /* INC_FREERTOS_H */")?;
    writeln!(out)?;
    writeln!(out, "#ifndef configSTACK_DEPTH_TYPE")?;
    writeln!(out, "    #define configSTACK_DEPTH_TYPE uint32_t")?;
    writeln!(out, "#endif")?;
    writeln!(out)?;
    writeln!(out, "#ifndef INC_TASK_H")?;
    writeln!(out, "struct tskTaskControlBlock;")?;
    writeln!(out, "typedef struct tskTaskControlBlock * TaskHandle_t;")?;
    writeln!(out, "typedef void (* TaskFunction_t)( void * );")?;
    writeln!(out, "#endif /* INC_TASK_H */")?;
    writeln!(out)?;
    writeln!(out, "#ifndef QUEUE_H")?;
    writeln!(out, "struct QueueDefinition;")?;
    writeln!(out, "typedef struct QueueDefinition * QueueHandle_t;")?;
    writeln!(out, "typedef struct QueueDefinition * QueueSetHandle_t;")?;
    writeln!(
        out,
        "typedef struct QueueDefinition * QueueSetMemberHandle_t;"
    )?;
    writeln!(out, "#endif /* QUEUE_H */")?;
    writeln!(out)?;
    writeln!(out, "#ifndef SEMAPHORE_H")?;
    writeln!(out, "typedef QueueHandle_t SemaphoreHandle_t;")?;
    writeln!(out, "#endif /* SEMAPHORE_H */")?;
    writeln!(out)?;
    writeln!(out, "#ifndef TIMERS_H")?;
    writeln!(out, "struct tmrTimerControl;")?;
    writeln!(out, "typedef struct tmrTimerControl * TimerHandle_t;")?;
    writeln!(
        out,
        "typedef void (* TimerCallbackFunction_t)( TimerHandle_t );"
    )?;
    writeln!(
        out,
        "typedef void (* PendedFunction_t)( void *, uint32_t );"
    )?;
    writeln!(out, "#endif /* TIMERS_H */")?;
    writeln!(out)?;
    writeln!(out, "#ifndef EVENT_GROUPS_H")?;
    writeln!(out, "struct EventGroupDef_t;")?;
    writeln!(out, "typedef struct EventGroupDef_t * EventGroupHandle_t;")?;
    writeln!(out, "typedef TickType_t EventBits_t;")?;
    writeln!(out, "#endif /* EVENT_GROUPS_H */")?;
    writeln!(out)?;
    writeln!(out, "#ifndef STREAM_BUFFER_H")?;
    writeln!(out, "struct StreamBufferDef_t;")?;
    writeln!(
        out,
        "typedef struct StreamBufferDef_t * StreamBufferHandle_t;"
    )?;
    writeln!(
        out,
        "typedef void (* StreamBufferCallbackFunction_t)( StreamBufferHandle_t, BaseType_t, BaseType_t * );"
    )?;
    writeln!(out, "#endif /* STREAM_BUFFER_H */")?;
    writeln!(out)?;
    writeln!(out, "#ifndef pdPASS")?;
    writeln!(out, "    #define pdFALSE ( ( BaseType_t ) 0 )")?;
    writeln!(out, "    #define pdTRUE  ( ( BaseType_t ) 1 )")?;
    writeln!(out, "    #define pdFAIL  ( pdFALSE )")?;
    writeln!(out, "    #define pdPASS  ( pdTRUE )")?;
    writeln!(out, "#endif")
}

#[cfg(test)]
// The house lint policy (H-15..H-18) bans panicking APIs and bare
// indexing on every path. A test IS the path where a panic is the
// report, so tests opt out per file, as every Kairos package does.
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]
mod tests {
    use super::*;
    extern crate alloc;
    use alloc::format;
    use alloc::string::String;

    fn header() -> String {
        let mut s = String::new();
        write_header(&mut s).expect("a String never fails to write");
        s
    }

    #[test]
    fn the_derived_surface_is_the_size_the_derivation_said() {
        // 108 undefined symbols across 33 demo files, less 11 `__aeabi_*`
        // helpers and 6 board drivers, is about 84. A table that has
        // drifted far from that has stopped being the derived list and has
        // become somebody's idea of a nice API.
        assert!(
            (80..=100).contains(&SYMBOLS.len()),
            "{} symbols -- re-derive with `llvm-nm -u` before widening this",
            SYMBOLS.len()
        );
    }

    #[test]
    fn no_symbol_is_declared_twice() {
        for (i, a) in SYMBOLS.iter().enumerate() {
            for b in &SYMBOLS[i + 1..] {
                assert_ne!(a.name, b.name, "{} declared twice", a.name);
            }
        }
    }

    #[test]
    fn every_symbol_is_a_c_identifier_with_a_return_type() {
        for s in SYMBOLS {
            assert!(!s.name.is_empty());
            assert!(
                s.name
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_'),
                "{}",
                s.name
            );
            assert!(!s.ret.is_empty(), "{} has no return type", s.name);
        }
    }

    /// A parameter list written `""` generates `f( )`, which in C is an
    /// UNPROTOTYPED function that accepts anything -- the one way a
    /// generated header can be worse than no header at all.
    #[test]
    fn a_no_argument_function_says_void_rather_than_nothing() {
        for s in SYMBOLS {
            assert!(!s.args.trim().is_empty(), "{}", s.name);
        }
    }

    #[test]
    fn the_header_declares_every_symbol_exactly_once() {
        let h = header();
        for s in SYMBOLS {
            let decl = format!("{} {}(", s.ret, s.name);
            assert_eq!(
                h.matches(&decl).count(),
                1,
                "{} missing from or duplicated in the header",
                s.name
            );
        }
    }

    #[test]
    fn the_header_has_an_include_guard_and_closes_it() {
        let h = header();
        assert!(h.contains("#ifndef KAIROS_CAPI_H"));
        assert!(h.contains("#define KAIROS_CAPI_H"));
        assert!(h.trim_end().ends_with("#endif /* KAIROS_CAPI_H */"));
    }

    #[test]
    fn the_header_is_deterministic() {
        assert_eq!(header(), header());
    }

    /// Every typedef group must stand aside for the FreeRTOS header that
    /// owns it. A group without its guard is a hard compile error for the
    /// program this header exists to serve.
    #[test]
    fn every_typedef_group_defers_to_the_header_that_owns_it() {
        let h = header();
        for guard in [
            "INC_FREERTOS_H",
            "INC_TASK_H",
            "QUEUE_H",
            "SEMAPHORE_H",
            "TIMERS_H",
            "EVENT_GROUPS_H",
            "STREAM_BUFFER_H",
        ] {
            assert!(
                h.contains(&format!("#ifndef {guard}")),
                "the typedefs owned by {guard} are unguarded"
            );
        }
        assert!(h.contains("#ifndef pdPASS"));
        assert!(h.contains("#ifndef configSTACK_DEPTH_TYPE"));
    }

    /// `void *` is the right ABI and the wrong declaration: FreeRTOS
    /// spells its handles as incomplete struct pointers, and a header that
    /// disagrees cannot be included beside theirs.
    #[test]
    fn handles_are_declared_the_shape_freertos_declares_them() {
        let h = header();
        assert!(h.contains("typedef struct QueueDefinition * QueueHandle_t;"));
        assert!(h.contains("typedef struct tskTaskControlBlock * TaskHandle_t;"));
        assert!(
            !h.contains("typedef void * QueueHandle_t;"),
            "a `void *` handle typedef conflicts with queue.h"
        );
    }

    #[test]
    fn the_eight_symbols_pollq_derived_are_all_still_here() {
        // The first cell's whole surface. If any of these ever leaves the
        // table, the derivation has been edited rather than re-run.
        for want in [
            "xTaskCreate",
            "vTaskDelay",
            "xQueueGenericCreate",
            "xQueueGenericSend",
            "xQueueReceive",
            "uxQueueMessagesWaiting",
            "vPortEnterCritical",
            "vPortExitCritical",
        ] {
            assert!(
                SYMBOLS.iter().any(|s| s.name == want),
                "{want} is gone from the derived surface"
            );
        }
    }
}
