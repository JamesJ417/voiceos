package dev.voiceos.client

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class TtsTerminalCompletionGateTest {
    @Test
    fun onlyFirstTerminalCallbackCompletesAnUtterance() {
        val gate = TtsTerminalCompletionGate()

        assertTrue(gate.tryComplete())
        assertFalse(gate.tryComplete())

        gate.reset()
        assertTrue(gate.tryComplete())
    }
}
