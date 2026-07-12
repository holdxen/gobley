/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/.
 */

import io.kotest.matchers.shouldBe
import set_and_box.*
import kotlin.test.Test

class SetAndBoxTest {
    @Test
    fun testStringSetIdentity() {
        val set = makeStringSet(listOf("a", "b", "c"))
        stringSetIdentity(set) shouldBe set
    }

    @Test
    fun testBoxU32Identity() {
        boxU32Identity(42U) shouldBe 42U
    }
}
