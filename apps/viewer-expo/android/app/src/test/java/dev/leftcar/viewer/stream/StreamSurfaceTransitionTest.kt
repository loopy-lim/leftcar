package dev.leftcar.viewer.stream

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertNull
import org.junit.Assert.assertTrue
import org.junit.Test

/**
 * Regression for the U3 live-device failure: a 4K splitVertical promotion
 * intent on an already-visible single-surface StreamActivity window detached
 * the old renderer ("Transient Surface detach ... src-5003") and prepared the
 * split UDP listeners (5003/5004), but the onNewIntent branch only bumped the
 * surface generation and waited for SurfaceHolder callbacks that could never
 * arrive — the view hierarchy was still the single-SurfaceView one built in
 * onCreate, so attachSplitSurfaces never ran, no receiver feedback reached the
 * Host, and the Host errored after ~6s with "viewer connection lost
 * (feedback timeout)".
 *
 * The decision tests pin the onNewIntent branch for every transition; the
 * gate tests drive the actual SurfaceHolder callback lifecycle — create →
 * debounce attach, destroy → detach/final-release, retired-hierarchy
 * destroys — through the exact bookkeeping StreamActivity executes. Since the
 * eager pre-swap detach model, rebuildStreamSurfaces stops the replaced
 * renderer BEFORE hierarchySwapped, and retired holders' destroys are
 * fully inert in ANY arrival order — they can never detach a renderer the
 * new hierarchy attached.
 */
class StreamSurfaceTransitionTest {
    // ------------------------------------------------------------------
    // Intent transition decision (StreamActivity.onNewIntent)
    // ------------------------------------------------------------------

    @Test
    fun fourKPromotionOnALiveSingleWindowMustRebuildTheSurfaceHierarchy() {
        // Device reproduction: session 3 streaming 2560x1440 single on a valid
        // freeform Surface; DisplaySizeCard 4K → 3840x2160 splitVertical.
        val decision = decideSurfaceTransition(
            nextSplitVertical = true,
            hierarchyBuiltForSplit = false,
            leftSurfaceValid = true,
        )
        assertEquals(StreamSurfaceTransition.REBUILD_SURFACES, decision)
    }

    @Test
    fun sameModeSingleStreamStillRebindsOnTheValidSurface() {
        assertEquals(
            StreamSurfaceTransition.REBIND_IN_PLACE,
            decideSurfaceTransition(
                nextSplitVertical = false,
                hierarchyBuiltForSplit = false,
                leftSurfaceValid = true,
            ),
        )
    }

    @Test
    fun demotionFromSplitToSingleMustRebuildInsteadOfRebindingIntoTheOldHierarchy() {
        assertEquals(
            StreamSurfaceTransition.REBUILD_SURFACES,
            decideSurfaceTransition(
                nextSplitVertical = false,
                hierarchyBuiltForSplit = true,
                leftSurfaceValid = true,
            ),
        )
    }

    @Test
    fun reconnectOverAnInvalidSingleSurfaceMustRebuildInsteadOfWaitingForever() {
        assertEquals(
            StreamSurfaceTransition.REBUILD_SURFACES,
            decideSurfaceTransition(
                nextSplitVertical = false,
                hierarchyBuiltForSplit = false,
                leftSurfaceValid = false,
            ),
        )
    }

    // ------------------------------------------------------------------
    // SurfaceHolder callback lifecycle across the rebuild
    // ------------------------------------------------------------------

    @Test
    fun promotionReplacesHierarchyAndRetiredDestroysAreInert() {
        val gate = StreamSurfaceLifecycleGate<String>()
        gate.hierarchySwapped(listOf("left"))
        val firstAttach = gate.onSurfaceCreated("left")!!
        assertTrue(gate.canAttachStableSurfaces(firstAttach))
        gate.confirmAttached(true)
        assertTrue(gate.isAttached)

        // onNewIntent → rebuildStreamSurfaces: the Activity detached the old
        // renderer before this swap, then swapped to the pair. The retired
        // holder tracks nothing and the swap ends the attached state (the
        // pre-swap detach did the stopping).
        gate.hierarchySwapped(listOf("splitL", "splitR"))
        assertTrue(gate.tracks("splitL") && gate.tracks("splitR"))
        assertFalse(gate.tracks("left"))
        assertFalse(gate.isAttached)

        // The replaced holder's late destroy is fully inert: no stop decision
        // (the pre-swap detach already stopped it) and no bookkeeping noise.
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("left", false))

        // The new hierarchy's debounce attach still completes with both split
        // holders despite the interleaved retired destroy.
        assertNull(gate.onSurfaceCreated("splitL"))
        val scheduled = gate.onSurfaceCreated("splitR")!!
        assertTrue(gate.canAttachStableSurfaces(scheduled))
        gate.confirmAttached(true)
        assertTrue(gate.isAttached)

        // And even a destroy repeating after this fresh attach cannot detach
        // it through the retired holder.
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("left", false))
        assertTrue(gate.isAttached)
    }

    @Test
    fun retiredDestroyLandingAfterTheSplitAttachCannotDetachTheFreshRenderer() {
        val gate = StreamSurfaceLifecycleGate<String>()
        gate.hierarchySwapped(listOf("left"))
        val attach = gate.onSurfaceCreated("left")!!
        gate.confirmAttached(gate.canAttachStableSurfaces(attach))

        gate.hierarchySwapped(listOf("splitL", "splitR"))
        assertNull(gate.onSurfaceCreated("splitL"))
        val scheduled = gate.onSurfaceCreated("splitR")!!
        gate.confirmAttached(gate.canAttachStableSurfaces(scheduled))
        assertTrue(gate.isAttached)

        // Device session-3 sequence: the old single holder's destroy races in
        // AFTER the split renderer attached. The retired holder owns nothing,
        // so the fresh split renderer survives untouched.
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("left", false))
        assertTrue(gate.isAttached)
    }

    @Test
    fun retiredHolderThatNeverHostedARendererIsIgnoredSilently() {
        val gate = StreamSurfaceLifecycleGate<String>()
        gate.hierarchySwapped(listOf("old"))
        gate.hierarchySwapped(listOf("newL", "newR"))
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("old", false))
        assertFalse(gate.tracks("old"))
    }

    @Test
    fun longLivedActivityDoesNotAccumulateRetiredHoldersAcrossRebuilds() {
        val gate = StreamSurfaceLifecycleGate<String>()
        var current = listOf("a")
        gate.hierarchySwapped(current)
        // Each rebuild forgets the previous holders outright; nothing from a
        // replaced hierarchy is retained for the Activity's lifetime.
        current = listOf("b1", "b2")
        gate.hierarchySwapped(current)
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("a", false))
        current = listOf("c")
        gate.hierarchySwapped(current)
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("b1", false))
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("b2", false))
        assertFalse(gate.tracks("a") || gate.tracks("b1") || gate.tracks("b2"))
    }

    @Test
    fun liveResizeDestroyDetachesWhileAFinalFinishReleasesExactlyOnce() {
        val gate = StreamSurfaceLifecycleGate<String>()
        gate.hierarchySwapped(listOf("single"))
        val attach = gate.onSurfaceCreated("single")!!
        gate.confirmAttached(gate.canAttachStableSurfaces(attach))
        assertTrue(gate.isAttached)

        // Desktop-mode resize: Surface destroyed, Activity stays alive.
        assertEquals(StreamSurfaceStop.DETACH_RENDERER, gate.onSurfaceDestroyed("single", false))
        assertFalse(gate.isAttached)
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("single", false))

        // Surface recreation re-arms the attach from the fresh generation.
        val rescheduled = gate.onSurfaceCreated("single")!!
        assertTrue(gate.canAttachStableSurfaces(rescheduled))
        assertFalse(gate.canAttachStableSurfaces(attach))
        gate.confirmAttached(true)

        // Final finish destroy releases once; any repeat is inert.
        assertEquals(StreamSurfaceStop.FINAL_RELEASE, gate.onSurfaceDestroyed("single", true))
        assertFalse(gate.isAttached)
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("single", true))
    }

    @Test
    fun aDestroyInsideTheDebounceWindowCancelsThePendingAttach() {
        val gate = StreamSurfaceLifecycleGate<String>()
        gate.hierarchySwapped(listOf("l", "r"))
        assertNull(gate.onSurfaceCreated("l"))
        val scheduled = gate.onSurfaceCreated("r")!!

        // r is destroyed again inside the 300ms debounce (resize storm).
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("r", false))
        assertFalse(gate.canAttachStableSurfaces(scheduled))

        // r returns; the debounce re-arms from the fresh generation only.
        val rescheduled = gate.onSurfaceCreated("r")!!
        assertTrue(gate.canAttachStableSurfaces(rescheduled))
        assertFalse(gate.canAttachStableSurfaces(scheduled))
    }

    @Test
    fun onlyTheLiveAttachedHierarchyFeedsGeometryUpdates() {
        val gate = StreamSurfaceLifecycleGate<String>()
        gate.hierarchySwapped(listOf("left"))
        gate.onSurfaceCreated("left")
        assertFalse(gate.shouldUpdateGeometry("left"))

        val attach = 2 // generation after swap(1) + create(2)
        gate.confirmAttached(gate.canAttachStableSurfaces(attach))
        assertTrue(gate.shouldUpdateGeometry("left"))

        gate.hierarchySwapped(listOf("splitL", "splitR"))
        assertFalse(gate.shouldUpdateGeometry("left"))
        // The real rebuild sequence stops the old renderer via the pre-swap
        // detach; the retired holder's destroy is inert and grants no
        // geometry either.
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("left", false))
        assertFalse(gate.shouldUpdateGeometry("splitL"))

        assertNull(gate.onSurfaceCreated("splitL"))
        val scheduled = gate.onSurfaceCreated("splitR")!!
        gate.confirmAttached(gate.canAttachStableSurfaces(scheduled))
        assertTrue(gate.shouldUpdateGeometry("splitL"))
        assertFalse(gate.shouldUpdateGeometry("left"))
    }

    @Test
    fun rapidSingleSplitRoundTripKeepsEveryAttachIntact() {
        val gate = StreamSurfaceLifecycleGate<String>()
        // 1440p single, live.
        gate.hierarchySwapped(listOf("s1"))
        gate.confirmAttached(
            gate.canAttachStableSurfaces(gate.onSurfaceCreated("s1")!!),
        )
        assertTrue(gate.isAttached)

        // Promotion: pre-swap detach, then the split pair. Both old holders'
        // destroys arrive in either order — even after the new attach.
        gate.hierarchySwapped(listOf("l1", "r1"))
        gate.onSurfaceDestroyed("s1", false)
        assertNull(gate.onSurfaceCreated("l1"))
        val splitAttach = gate.onSurfaceCreated("r1")!!
        gate.confirmAttached(gate.canAttachStableSurfaces(splitAttach))
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("s1", false))
        assertTrue(gate.isAttached)

        // Demotion back to single, immediately. The retired split holders'
        // destroys land interleaved with the fresh single holder's create.
        gate.hierarchySwapped(listOf("s2"))
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("l1", false))
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("r1", false))
        val singleAttach = gate.onSurfaceCreated("s2")!!
        assertTrue(gate.canAttachStableSurfaces(singleAttach))
        gate.confirmAttached(true)
        assertTrue(gate.isAttached)
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("l1", false))
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("r1", false))
        assertTrue(gate.isAttached)
    }

    @Test
    fun demotionWithLateRetiredSplitDestroysAttachesTheSingleRenderer() {
        val gate = StreamSurfaceLifecycleGate<String>()
        gate.hierarchySwapped(listOf("l1", "r1"))
        assertNull(gate.onSurfaceCreated("l1"))
        val splitAttach = gate.onSurfaceCreated("r1")!!
        gate.confirmAttached(gate.canAttachStableSurfaces(splitAttach))
        assertTrue(gate.isAttached)

        // Demotion: pre-swap detach (Activity), swap, then BOTH old split
        // holders' first destroys land late — the left one even after the
        // new single attach. None may detach the fresh renderer.
        gate.hierarchySwapped(listOf("s2"))
        val firstRetired = gate.onSurfaceDestroyed("l1", false)
        val singleAttach = gate.onSurfaceCreated("s2")!!
        gate.confirmAttached(gate.canAttachStableSurfaces(singleAttach))
        assertEquals(StreamSurfaceStop.NONE, firstRetired)
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("r1", false))
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("l1", false))
        assertTrue(gate.isAttached)
    }

    @Test
    fun liveResizeStillDetachesAndRearmsWithinTheSameHierarchy() {
        val gate = StreamSurfaceLifecycleGate<String>()
        gate.hierarchySwapped(listOf("single"))
        val attach = gate.onSurfaceCreated("single")!!
        gate.confirmAttached(gate.canAttachStableSurfaces(attach))
        assertTrue(gate.isAttached)

        // Tracked-holder destroy still owns the live renderer.
        assertEquals(StreamSurfaceStop.DETACH_RENDERER, gate.onSurfaceDestroyed("single", false))
        assertFalse(gate.isAttached)
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("single", false))
        val rescheduled = gate.onSurfaceCreated("single")!!
        assertTrue(gate.canAttachStableSurfaces(rescheduled))
        gate.confirmAttached(true)
        assertTrue(gate.isAttached)
    }

    @Test
    fun invalidateOnDestroyStopsAnyPendingAttach() {
        val gate = StreamSurfaceLifecycleGate<String>()
        gate.hierarchySwapped(listOf("single"))
        val scheduled = gate.onSurfaceCreated("single")!!
        assertTrue(gate.canAttachStableSurfaces(scheduled))

        gate.invalidate()
        assertFalse(gate.canAttachStableSurfaces(scheduled))
    }

    @Test
    fun splitLiveResizeDetachesOnceWhileThePeerDestroyIsInert() {
        val gate = StreamSurfaceLifecycleGate<String>()
        gate.hierarchySwapped(listOf("l", "r"))
        assertNull(gate.onSurfaceCreated("l"))
        val splitAttach = gate.onSurfaceCreated("r")!!
        gate.confirmAttached(gate.canAttachStableSurfaces(splitAttach))
        assertTrue(gate.isAttached)

        // Split live resize (freeform drag): the framework destroys BOTH
        // holders. The first tracked destroy owns the renderer stop; the
        // second must not double-stop or re-arm anything.
        assertEquals(StreamSurfaceStop.DETACH_RENDERER, gate.onSurfaceDestroyed("l", false))
        assertFalse(gate.isAttached)
        assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed("r", false))

        // Both halves return; the attach re-arms only with the fresh pair.
        assertNull(gate.onSurfaceCreated("l"))
        val rescheduled = gate.onSurfaceCreated("r")!!
        assertTrue(gate.canAttachStableSurfaces(rescheduled))
        assertFalse(gate.canAttachStableSurfaces(splitAttach))
        gate.confirmAttached(true)
        assertTrue(gate.isAttached)
    }

    @Test
    fun rebuildDuringTheDebounceWindowCancelsTheOldHierarchyAttach() {
        val gate = StreamSurfaceLifecycleGate<String>()
        gate.hierarchySwapped(listOf("s1"))
        val staleScheduled = gate.onSurfaceCreated("s1")!!

        // Promotion intent arrives while the 300ms debounce for s1 is still
        // pending: the pre-swap detach + hierarchySwapped must retire that
        // attach forever.
        gate.hierarchySwapped(listOf("l1", "r1"))
        assertFalse(gate.canAttachStableSurfaces(staleScheduled))

        // The late create of the retired holder must not re-arm anything
        // either (it tracks nothing).
        assertNull(gate.onSurfaceCreated("s1"))
        assertFalse(gate.canAttachStableSurfaces(staleScheduled))

        assertNull(gate.onSurfaceCreated("l1"))
        val fresh = gate.onSurfaceCreated("r1")!!
        assertTrue(gate.canAttachStableSurfaces(fresh))
        gate.confirmAttached(true)
        assertTrue(gate.isAttached)
    }

    @Test
    fun doubleRoundTripKeepsAttachesAliveThroughFourModeChanges() {
        val gate = StreamSurfaceLifecycleGate<String>()

        fun promoteOrDemote(next: List<String>, retired: List<String>) {
            gate.hierarchySwapped(next)
            // Every retired holder's destroy is inert — before, interleaved
            // with, or after the fresh attach.
            retired.forEach { holder ->
                assertEquals(StreamSurfaceStop.NONE, gate.onSurfaceDestroyed(holder, false))
            }
        }

        fun attachFresh(holders: List<String>) {
            val last = holders.last()
            holders.forEach { holder ->
                val scheduled = gate.onSurfaceCreated(holder)
                if (holder == last) {
                    gate.confirmAttached(gate.canAttachStableSurfaces(scheduled!!))
                }
            }
            assertTrue(gate.isAttached)
        }

        // 1440p single → 4K split → 1440p single → 4K split, the exact P0-8
        // device matrix, with retired destroys trailing each transition.
        gate.hierarchySwapped(listOf("s1"))
        attachFresh(listOf("s1"))

        promoteOrDemote(listOf("l1", "r1"), listOf("s1"))
        attachFresh(listOf("l1", "r1"))

        promoteOrDemote(listOf("s2"), listOf("l1", "r1"))
        attachFresh(listOf("s2"))

        promoteOrDemote(listOf("l2", "r2"), listOf("s2"))
        attachFresh(listOf("l2", "r2"))

        assertFalse(gate.tracks("s1") || gate.tracks("l1") || gate.tracks("r1") || gate.tracks("s2"))
    }
}
