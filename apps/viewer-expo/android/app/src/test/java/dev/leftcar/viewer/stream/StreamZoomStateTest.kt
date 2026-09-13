package dev.leftcar.viewer.stream

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class StreamZoomStateTest {
    @Test
    fun `focus anchoring keeps the content point under the fingers`() {
        val zoom = StreamZoomState()
        // 뷰 1000×2000, 포커스 (300, 400)에서 2배 확대.
        zoom.applyScale(2f, 300f, 400f, viewWidth = 1000, viewHeight = 2000)
        // 줌 후 뷰 좌표 (300, 400)의 내용 좌표는 줌 전과 같아야 한다.
        val (cx, cy) = zoom.toContent(300f, 400f)
        val (bx, by) = StreamZoomState().toContent(300f, 400f)
        assertEquals(bx, cx, 0.01f)
        assertEquals(by, cy, 0.01f)
        assertTrue(zoom.isZoomed)
        assertEquals(2f, zoom.scale, 0.01f)
    }

    @Test
    fun `translation is clamped so content always covers the view`() {
        val zoom = StreamZoomState()
        zoom.applyScale(2f, 0f, 0f, viewWidth = 1000, viewHeight = 1000)
        // 원점 기준 2배: tx = 0 - (0-0)*2 = 0 → 클램프 상한 0에 걸린다.
        assertEquals(0f, zoom.translationX, 0.01f)
        assertEquals(0f, zoom.translationY, 0.01f)
        // 우하단 포커스 확대: tx는 (1-scale)*w 하한에 걸린다.
        val zoom2 = StreamZoomState()
        zoom2.applyScale(3f, 1000f, 1000f, viewWidth = 1000, viewHeight = 1000)
        assertEquals(-2000f, zoom2.translationX, 0.01f)
        assertEquals(-2000f, zoom2.translationY, 0.01f)
    }

    @Test
    fun `toContent round-trips through the zoom transform`() {
        val zoom = StreamZoomState()
        zoom.applyScale(2.5f, 250f, 260f, viewWidth = 800, viewHeight = 600)
        val (cx, cy) = zoom.toContent(500f, 300f)
        // 뷰 좌표 = 내용 × scale + t 역산이 정확한지 확인.
        assertEquals(500f, cx * zoom.scale + zoom.translationX, 0.01f)
        assertEquals(300f, cy * zoom.scale + zoom.translationY, 0.01f)
    }

    @Test
    fun `scale caps at the maximum and snaps out near identity`() {
        val zoom = StreamZoomState()
        repeat(20) { zoom.applyScale(2f, 100f, 100f, viewWidth = 500, viewHeight = 500) }
        assertEquals(StreamZoomState.MAX_SCALE, zoom.scale, 0.01f)

        // 핀치를 1 아래로 닫으면 스냅아웃(원상 복귀)한다.
        val closing = StreamZoomState()
        closing.applyScale(1.5f, 100f, 100f, viewWidth = 500, viewHeight = 500)
        assertTrue(closing.isZoomed)
        closing.applyScale(0.6f, 100f, 100f, viewWidth = 500, viewHeight = 500)
        assertFalse(closing.isZoomed)
        assertEquals(1f, closing.scale, 0.001f)
        assertEquals(0f, closing.translationX, 0.001f)
        assertEquals(0f, closing.translationY, 0.001f)
    }

    @Test
    fun `reset returns a zoomed state to identity for surface rebuilds`() {
        // 서피스 재구성(rebuildStreamSurfaces)은 변환 없는 새 뷰를 만드므로
        // 상태도 원점이어야 한다 — 남은 줌은 입력 역산을 어긋나게 한다.
        val zoom = StreamZoomState()
        zoom.applyScale(3f, 200f, 150f, viewWidth = 800, viewHeight = 600)
        assertTrue(zoom.isZoomed)
        zoom.reset()
        assertFalse(zoom.isZoomed)
        assertEquals(1f, zoom.scale, 0.0001f)
        assertEquals(0f, zoom.translationX, 0.0001f)
        assertEquals(0f, zoom.translationY, 0.0001f)
        val (cx, cy) = zoom.toContent(123f, 456f)
        assertEquals(123f, cx, 0.0001f)
        assertEquals(456f, cy, 0.0001f)
    }

    @Test
    fun `seam pivots anchor each split tile at the shared edge`() {
        // 왼쪽 타일은 오른쪽 모서리(이음선), 오른쪽 타일은 왼쪽 모서리 —
        // 같은 배율·이동에서 두 타일이 이음선에서 이어진다.
        assertEquals(480f, StreamZoomState.seamPivotX(leftTile = true, tileWidth = 480), 0.0001f)
        assertEquals(0f, StreamZoomState.seamPivotX(leftTile = false, tileWidth = 480), 0.0001f)
    }
}
