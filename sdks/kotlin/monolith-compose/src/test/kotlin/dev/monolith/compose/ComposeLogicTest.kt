package dev.monolith.compose

import dev.monolith.client.auth.AppUser
import org.junit.Assert.assertEquals
import org.junit.Test

class ComposeLogicTest {
    @Test
    fun knownProvidersUseTheirBrandName() {
        assertEquals("GitHub", ProviderBrands.forProvider("github", "whatever").name)
        assertEquals("Google", ProviderBrands.forProvider("GOOGLE", "x").name)
    }

    @Test
    fun unknownProvidersFallBackToTheServerName() {
        val b = ProviderBrands.forProvider("oidc", "Acme SSO")
        assertEquals("Acme SSO", b.name)
        assertEquals("A", b.letter)
        assertEquals("zed", ProviderBrands.forProvider("zed", "").name)
    }

    @Test
    fun continueLabelIsOverridable() {
        assertEquals("Continue with Google", AuthLabels().continueWith("Google"))
        assertEquals("Weiter mit Google", AuthLabels(continueWith = { "Weiter mit $it" }).continueWith("Google"))
    }

    @Test
    fun userInitialPrefersNameThenEmail() {
        assertEquals("A", userInitial(AppUser(id = "1", email = "z@x.io", name = "ann")))
        assertEquals("Z", userInitial(AppUser(id = "1", email = "z@x.io", name = " ")))
        assertEquals("?", userInitial(AppUser(id = "1", email = null, name = "")))
    }
}
