package dev.leftcar.viewer.stream

import android.app.Activity
import android.content.Intent
import android.animation.ValueAnimator
import android.graphics.Color
import android.graphics.drawable.GradientDrawable
import android.os.Looper
import android.view.View
import android.widget.LinearLayout
import android.widget.PopupWindow
import android.widget.ProgressBar
import android.widget.TextView
import org.junit.Assert.*
import org.junit.Test
import org.junit.runner.RunWith
import org.robolectric.Robolectric
import org.robolectric.RobolectricTestRunner
import org.robolectric.Shadows.shadowOf
import org.robolectric.annotation.Config
import org.robolectric.util.ReflectionHelpers
import org.robolectric.util.ReflectionHelpers.ClassParameter.from
import androidx.core.graphics.ColorUtils
import java.util.concurrent.TimeUnit

@RunWith(RobolectricTestRunner::class)
@Config(sdk = [35], manifest = Config.NONE, shadows = [PresentationNativeShadow::class])
class StreamHudAccessibilityTest {
    @Test fun `hidden diagnostics still notify recovery when rendered frames advance`() {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        var progress = 0
        val hud = StreamHudController(controller.get(), "hidden-diagnostics", 60, false, {},
            onRenderedFrame = { progress += 1 })
        try {
            PresentationNativeShadow.renderedFrames = 100L
            hud.show()
            shadowOf(Looper.getMainLooper()).idle()
            PresentationNativeShadow.renderedFrames = 101L
            shadowOf(Looper.getMainLooper()).idleFor(250, TimeUnit.MILLISECONDS)
            assertEquals(1, progress)
            assertNull(ReflectionHelpers.getField<TextView?>(hud, "statsView"))
            assertNull(ReflectionHelpers.getField<PopupWindow?>(hud, "statsPopup"))
        } finally {
            hud.stop()
            PresentationNativeShadow.renderedFrames = 0L
            controller.destroy()
        }
    }

    @Test fun `locked input exposes a named native click action with a 44dp target`() {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        val activity = controller.get()
        var requests = 0
        val hud = StreamHudController(activity, "input-action", 60, false, {}, onInputRequest = { requests += 1; true })
        hud.show()
        shadowOf(Looper.getMainLooper()).idle()
        ReflectionHelpers.callInstanceMethod<Void>(hud, "updateInput", from(Int::class.javaPrimitiveType!!, 0))
        val badge = ReflectionHelpers.getField<View>(hud, "inputView")
        val background = (badge.background as GradientDrawable).color!!.defaultColor
        assertEquals("Video content must not change text contrast", 255, Color.alpha(background))
        assertTrue(ColorUtils.calculateContrast(ReflectionHelpers.getField<TextView>(hud, "inputLabel").currentTextColor, background) >= 4.5)
        val node = badge.createAccessibilityNodeInfo()
        assertEquals("android.widget.Button", node.className.toString())
        assertTrue(node.isClickable)
        assertTrue(node.contentDescription.toString().contains("허용 요청"))
        assertTrue(badge.minimumHeight / activity.resources.displayMetrics.density >= 44f)
        assertTrue(badge.performClick())
        assertEquals(1, requests)
        badge.performClick()
        assertEquals("Repeated native clicks must use the same cooldown", 1, requests)
        val popup = ReflectionHelpers.getField<PopupWindow>(hud, "inputPopup")
        assertFalse("The approval popup must not steal stream Back or keyboard routing", popup.isFocusable)
        assertFalse("Touches outside the approval target must still reach the video", popup.isTouchModal)
        ReflectionHelpers.callInstanceMethod<Void>(hud, "updateInput", from(Int::class.javaPrimitiveType!!, 1))
        badge.performClick()
        assertEquals("The approval action must not run while input is enabled", 1, requests)
        assertFalse(popup.isTouchable)
        assertFalse(popup.isFocusable)
        hud.stop()
        controller.destroy()
    }

    @Test fun `native input and diagnostics keep a readable minimum text size`() {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        val activity = controller.get()
        val hud = StreamHudController(activity, "readable-hud", 60, true, {})
        hud.show()
        val density = activity.resources.displayMetrics.scaledDensity
        assertTrue(ReflectionHelpers.getField<TextView>(hud, "inputLabel").textSize / density >= 12f)
        assertTrue(ReflectionHelpers.getField<TextView>(hud, "statsView").textSize / density >= 12f)
        hud.stop()
        controller.destroy()
    }

    @Test fun `failed reconnect has a local recovery action instead of a running spinner`() {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        var retries = 0
        val hud = StreamHudController(controller.get(), "rebind-failure", 60, false, {}, onRetryRebind = { retries += 1 })
        hud.showRebindIndicator(ViewerStrings.rebindReconnecting)
        hud.onRebindFinished(false)
        val row = ReflectionHelpers.getField<LinearLayout>(hud, "rebindView")
        val children = (0 until row.childCount).map(row::getChildAt)
        assertTrue(children.filterIsInstance<ProgressBar>().all { it.visibility != View.VISIBLE })
        assertTrue(children.any { it.isClickable && it.visibility == View.VISIBLE && !it.contentDescription.isNullOrBlank() })
        children.first { it.isClickable && it.visibility == View.VISIBLE }.performClick()
        assertEquals(1, retries)
        hud.stop()
        controller.destroy()
    }

    @Test fun `a queued HUD show cannot recreate a popup after stop`() {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        val hud = StreamHudController(controller.get(), "queued-stop", 60, false, {})
        hud.show()
        val queued = ReflectionHelpers.getField<PopupWindow>(hud, "inputPopup")
        hud.stop()
        shadowOf(Looper.getMainLooper()).idle()
        assertFalse(queued.isShowing)
        controller.destroy()
    }

    @Test fun `reduced motion retains progress text without a rotating spinner`() {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        val hud = StreamHudController(controller.get(), "reduced-motion", 60, false, {})
        ReflectionHelpers.callStaticMethod<Void>(ValueAnimator::class.java, "setDurationScale", from(Float::class.javaPrimitiveType!!, 0f))
        try {
            hud.showRebindIndicator(ViewerStrings.rebindReconnecting)
            val spinner = ReflectionHelpers.getField<ProgressBar>(hud, "rebindSpinner")
            assertEquals(View.GONE, spinner.visibility)
            assertEquals(ViewerStrings.rebindReconnecting, ReflectionHelpers.getField<TextView>(hud, "rebindText").text.toString())
        } finally {
            ReflectionHelpers.callStaticMethod<Void>(ValueAnimator::class.java, "setDurationScale", from(Float::class.javaPrimitiveType!!, 1f))
            hud.stop()
            controller.destroy()
        }
    }

    @Test fun `dismissed gesture help does not appear after its queued show`() {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        val overlay = GestureHintOverlay(controller.get()) {}
        overlay.show()
        val queued = ReflectionHelpers.getField<PopupWindow>(overlay, "popup")
        overlay.dismiss()
        shadowOf(Looper.getMainLooper()).idle()
        assertFalse(queued.isShowing)
        controller.destroy()
    }

    @Test fun `foreground request failures stay local and stale request replies cannot overwrite retry progress`() {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        var requestId = ""
        val hud = StreamHudController(controller.get(), "foreground-failure", 60, false, {},
            onInputRequest = { requestId = it; true })
        hud.show()
        shadowOf(Looper.getMainLooper()).idle()
        val badge = ReflectionHelpers.getField<View>(hud, "inputView")
        badge.performClick()
        assertFalse(badge.isEnabled)
        val firstRequest = requestId
        assertFalse(hud.onInputRequestResult("another-request", "unrelated error"))
        assertTrue(hud.onInputRequestResult(firstRequest, "Host request failed"))
        assertTrue(badge.contentDescription.toString().contains("Host request failed"))
        assertTrue(badge.contentDescription.toString().contains(ViewerStrings.inputRequestRetry))
        assertTrue(badge.isEnabled)
        shadowOf(Looper.getMainLooper()).idleFor(3, TimeUnit.SECONDS)
        badge.performClick()
        assertNotEquals(firstRequest, requestId)
        assertFalse(hud.onInputRequestResult(firstRequest, "late error"))
        assertEquals(ViewerStrings.inputRequestPending, badge.contentDescription)
        hud.invalidateInputRequests()
        assertFalse(hud.onInputRequestResult(requestId, "old generation error"))
        hud.stop()
        controller.destroy()
    }

    @Test fun `missing JS delivery times out into a named native retry without enabling input`() {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        val hud = StreamHudController(controller.get(), "foreground-timeout", 60, false, {}, onInputRequest = { true })
        hud.show()
        shadowOf(Looper.getMainLooper()).idle()
        val badge = ReflectionHelpers.getField<View>(hud, "inputView")
        badge.performClick()
        shadowOf(Looper.getMainLooper()).idleFor(8, TimeUnit.SECONDS)
        assertTrue(badge.isEnabled)
        assertTrue(badge.contentDescription.toString().contains(ViewerStrings.inputRequestTimeout))
        assertTrue(badge.contentDescription.toString().contains(ViewerStrings.inputRequestRetry))
        assertEquals(0, ReflectionHelpers.getField<Int>(hud, "lastInputStatus"))
        hud.stop()
        controller.destroy()
    }

    @Test fun `recreated HUDs sharing a logical stream generation never reuse a request id`() {
        val controller = Robolectric.buildActivity(Activity::class.java).setup()
        var originalId = ""
        val original = StreamHudController(controller.get(), "retained-instance", 60, false, {},
            onInputRequest = { originalId = it; true })
        original.show()
        shadowOf(Looper.getMainLooper()).idle()
        ReflectionHelpers.getField<View>(original, "inputView").performClick()
        original.stop()
        var recreatedId = ""
        val recreated = StreamHudController(controller.get(), "retained-instance", 60, false, {},
            onInputRequest = { recreatedId = it; true })
        recreated.show()
        shadowOf(Looper.getMainLooper()).idle()
        val currentBadge = ReflectionHelpers.getField<View>(recreated, "inputView")
        currentBadge.performClick()
        assertNotEquals(originalId, recreatedId)
        assertFalse(recreated.onInputRequestResult(originalId, "old physical Activity failure"))
        assertEquals(ViewerStrings.inputRequestPending, currentBadge.contentDescription)
        recreated.stop()
        controller.destroy()
    }
}
