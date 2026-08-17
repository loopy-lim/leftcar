package dev.leftcar.viewer.shim

import android.app.Activity
import android.content.Intent
import android.net.Uri
import android.os.Bundle
import android.widget.Button
import android.widget.LinearLayout
import android.widget.TextView
import android.view.Gravity

/**
 * Hub window (docs/03 §3.2): launcher that opens N independent StreamActivity
 * document tasks. Real multi-instance verification (H05): each open uses
 * FLAG_ACTIVITY_NEW_DOCUMENT | FLAG_ACTIVITY_MULTIPLE_TASK so the system
 * creates separate tasks — visible as separate windows in split-screen /
 * freeform / XR windowing.
 */
class HubActivity : Activity() {
    private var windowCount = 0

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            gravity = Gravity.CENTER
        }
        root.addView(TextView(this).apply {
            text = "Leftcar\nmulti-instance test"
            textSize = 24f
            gravity = Gravity.CENTER
            setPadding(0, 32, 0, 32)
        })
        val status = TextView(this).apply { text = "no windows yet"; gravity = Gravity.CENTER }
        fun openWindow() {
            val idx = windowCount++
            val instance = "instance-$idx-${System.nanoTime()}"
            val intent = Intent(this, StreamActivity::class.java).apply {
                data = Uri.parse("leftcar://stream/src-$idx?instance=$instance&idx=$idx")
                addFlags(Intent.FLAG_ACTIVITY_NEW_DOCUMENT)
                addFlags(Intent.FLAG_ACTIVITY_MULTIPLE_TASK)
            }
            startActivity(intent)
            status.text = "opened $windowCount windows"
        }
        root.addView(Button(this).apply {
            text = "Open stream window"
            setOnClickListener { openWindow() }
        })
        root.addView(Button(this).apply {
            text = "Open 4 windows"
            setOnClickListener { repeat(4) { openWindow() } }
        })
        root.addView(status)
        setContentView(root)
    }
}
