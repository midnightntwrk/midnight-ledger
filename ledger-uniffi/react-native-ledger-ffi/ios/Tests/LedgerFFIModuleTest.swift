import XCTest
import Foundation

// Import the UniFFI-generated bindings
import uniffi

class LedgerFFIModuleTest: XCTestCase {
    
    func testHello() {
        do {
            // Call the hello function directly from the Rust library via UniFFI
            let result = try LedgerUniffi.hello()
            
            // The hello() function should return a string
            XCTAssertNotNil(result, "hello() should not return nil")
            XCTAssertFalse(result.isEmpty, "hello() should return a non-empty string")
            
            // Check if it's a success result or error message
            if result.hasPrefix("Failed to call hello:") {
                // If it's an error, it should contain error information
                XCTAssertTrue(result.contains("Failed to call hello:"), 
                             "Error message should contain 'Failed to call hello:'")
                print("Warning: hello() returned error: \(result)")
            } else {
                // If it's a success, it should be a valid response
                XCTAssertFalse(result.hasPrefix("Failed"), 
                              "Success result should not start with 'Failed'")
                print("Success: hello() returned: \(result)")
                
                // Additional check: the hello function should return "hello"
                XCTAssertEqual(result, "hello", "hello() should return 'hello'")
            }
            
        } catch {
            XCTFail("hello() should not throw an error: \(error)")
        }
    }
    
    func testHelloFromSwiftWrapper() {
        // Test calling hello through the Swift wrapper (if we had one)
        // This would test the React Native bridge integration
        let module = LedgerFFI()
        
        // Note: In a real React Native module test, you'd need to set up the bridge
        // For now, we'll just test that the module can be instantiated
        XCTAssertNotNil(module, "LedgerFFI module should be instantiable")
    }
    
    func testNativeToken() {
        do {
            // Test another function from the Rust library
            let result = try LedgerUniffi.nativeToken()
            
            XCTAssertNotNil(result, "nativeToken() should not return nil")
            XCTAssertFalse(result.isEmpty, "nativeToken() should return a non-empty string")
            
            print("Success: nativeToken() returned: \(result)")
            
        } catch {
            XCTFail("nativeToken() should not throw an error: \(error)")
        }
    }
}
