package org.dash.sdk

import org.dash.sdk.services.DpnsService
import org.junit.Assert.assertEquals
import org.junit.Test

/**
 * Unit tests for [DpnsService.parseSearchResults]. Pure string parsing — no native library
 * or DAPI required, so these always run.
 */
class DpnsSearchParseTest {

    @Test
    fun parsesArrayOfObjectsToFullNames() {
        val json = """
            [
              {"label":"alice","fullName":"alice.dash","ownerId":"abc123"},
              {"label":"alicia","fullName":"alicia.dash","ownerId":"def456"}
            ]
        """.trimIndent()
        assertEquals(listOf("alice.dash", "alicia.dash"), DpnsService.parseSearchResults(json))
    }

    @Test
    fun fallsBackToLabelWhenFullNameMissing() {
        val json = """[{"label":"bob","ownerId":"x"}]"""
        assertEquals(listOf("bob"), DpnsService.parseSearchResults(json))
    }

    @Test
    fun parsesPlainStringArray() {
        val json = """["alice.dash","alice2.dash"]"""
        assertEquals(listOf("alice.dash", "alice2.dash"), DpnsService.parseSearchResults(json))
    }

    @Test
    fun emptyAndBlankInputsYieldEmptyList() {
        assertEquals(emptyList<String>(), DpnsService.parseSearchResults("[]"))
        assertEquals(emptyList<String>(), DpnsService.parseSearchResults("   "))
        assertEquals(emptyList<String>(), DpnsService.parseSearchResults(""))
    }
}
