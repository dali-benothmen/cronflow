//! Simple Bun FFI test functions
//! Based on Bun FFI documentation example

#[no_mangle]
pub extern "C" fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[no_mangle]
pub extern "C" fn multiply(a: i32, b: i32) -> i32 {
    a * b
}

#[no_mangle]
pub extern "C" fn test_string(input: *const i8) -> *const i8 {
    // Simple test - just return the input
    // In real implementation, we'd process the string
    input
}

#[no_mangle]
pub extern "C" fn workflow_test(workflow_json: *const i8) -> *const i8 {
    // Simulate our workflow registration
    // In real implementation, this would parse JSON and register workflow
    workflow_json
} 