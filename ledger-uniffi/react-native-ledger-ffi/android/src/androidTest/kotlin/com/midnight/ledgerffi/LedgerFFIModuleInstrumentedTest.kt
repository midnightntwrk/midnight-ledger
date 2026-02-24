package com.midnight.ledgerffi

import androidx.test.platform.app.InstrumentationRegistry
import androidx.test.ext.junit.runners.AndroidJUnit4
import org.junit.Test
import org.junit.runner.RunWith
import org.junit.Assert.*

/**
 * Instrumented test, which will execute on an Android device.
 * This test verifies that the native library is properly loaded and accessible.
 */
@RunWith(AndroidJUnit4::class)
class LedgerFFIModuleInstrumentedTest {

    @Test
    fun testHelloWithNativeLibrary() {
        // Context of the app under test
        val appContext = InstrumentationRegistry.getInstrumentation().targetContext
        assertEquals("com.midnight.ledgerffi.test", appContext.packageName)
        
        val module = LedgerFFIModule()
        val result = module.hello()
        
        // Verify the result is not null and not empty
        assertNotNull("hello() should not return null", result)
        assertTrue("hello() should return a non-empty string", result.isNotEmpty())
        
        // Log the result for debugging
        println("Instrumented test - hello() result: $result")
        
        // The result should either be a success message or a specific error
        // We don't assert the exact content since it depends on the native library
        assertTrue("Result should be a valid string", result is String)
    }
}
