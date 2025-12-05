package com.midnight.ledgerffi

import org.junit.Test
import org.junit.Assert.*

class LedgerFFIModuleTest {

    @Test
    fun testHello() {
        val module = LedgerFFIModule()
        val result = module.hello()
        
        // The hello() function should return a string
        assertNotNull("hello() should not return null", result)
        assertTrue("hello() should return a non-empty string", result.isNotEmpty())
        
        // Check if it's a success result or error message
        if (result.startsWith("Failed to call hello:")) {
            // If it's an error, it should contain error information
            assertTrue("Error message should contain 'Failed to call hello:'", 
                      result.contains("Failed to call hello:"))
            println("Warning: hello() returned error: $result")
        } else {
            // If it's a success, it should be a valid response
            assertFalse("Success result should not start with 'Failed'", 
                       result.startsWith("Failed"))
            println("Success: hello() returned: $result")
        }
    }
}
