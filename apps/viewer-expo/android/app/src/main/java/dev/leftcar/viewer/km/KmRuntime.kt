package dev.leftcar.viewer.km

import android.accessibilityservice.AccessibilityServiceInfo
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.os.Binder
import android.os.Handler
import android.os.HandlerThread
import android.os.IBinder
import android.os.Looper
import android.os.SystemClock
import android.util.Base64
import android.view.accessibility.AccessibilityManager
import org.json.JSONObject
import rikka.shizuku.Shizuku
import java.util.concurrent.CopyOnWriteArraySet
import java.util.concurrent.atomic.AtomicInteger
import kotlin.math.abs

internal data class KmNativeStatus(
    val state: String = "off", val reason: String? = null,
    val cursorInjection: Boolean = false, val mouseObservation: Boolean = false,
    val mouseCapture: Boolean = false, val keyboardCapture: Boolean = false,
)

internal data class KmStartConfig(
    val host: String, val port: Int, val sessionId: String, val sessionKey: String,
    val epoch: Long, val arrangementSide: String, val macToAndroid: Boolean, val androidToMac: Boolean,
) {
    fun validate() {
        require(host.isNotBlank() && host.length <= 253 && port in 1..65535)
        require(sessionId.toByteArray().size in 1..128 && epoch > 0)
        require(arrangementSide == "left" || arrangementSide == "right")
        require(macToAndroid || androidToMac)
    }
}

/** Single native actor owns input state. React receives low-rate status changes only. */
internal object KmRuntime {
    private class Unavailable(message: String) : IllegalStateException(message)
    private val thread = HandlerThread("leftcar-km-session").apply { start() }
    private val worker = Handler(thread.looper)
    private val main = Handler(Looper.getMainLooper())
    private var context: Context? = null
    private var authority: IKmInputAuthority? = null
    private var authorityBinder: IBinder? = null
    private var deadAuthorityBinder: IBinder? = null
    private var binding: ServiceConnection? = null
    @Volatile private var lease: KmKeyBridgeLease? = null
    private var args: Shizuku.UserServiceArgs? = null
    private var transport: KmDuplex? = null
    private var config: KmStartConfig? = null
    private var gate: KmEpochGate? = null
    private var outgoingSeq = 0L
    @Volatile private var generation = 0L
    @Volatile private var observationService: KmAccessibilityService? = null
    private val observationSession = KmObservationSession()
    private var observationSessionTicket = observationSession.ticket()
    private var lastReceived = 0L
    private var preparedAt = 0L
    private var owner = "local"
    private var motionX = 0.0
    private var motionY = 0.0
    private var sentButtons = 0
    private var lastCursor: KmObservedPointer? = null
    private var lastCursorSent = 0L
    private var returnArmed = false
    private var edgePending = false
    private var placement: KmCursorPlacement? = null
    private var remoteReleasePending = false
    private var returnPlacementReceived = false
    private var placementStarted = 0L
    private val delayedInput = ArrayDeque<JSONObject>()
    private val pending = AtomicInteger()
    private val starting = KmStartCompletion<KmNativeStatus>()
    private var startTicket = 0L
    @Volatile private var handshakeProbe: KmHandshakeProbe? = null
    private var handshakeStage = KmHandshakeProbe.Stage.PREPARE
    private var preparation: ((Result<KmNativeStatus>) -> Unit)? = null
    private val lifetime = Binder()
    private val transition = KmOwnerTransition(
        { startupStage(KmHandshakeProbe.Stage.STATE_NEUTRAL); authority!!.releaseAll() },
        { startupStage(KmHandshakeProbe.Stage.STATE_UNGRAB); authority!!.releaseMouse() }, ::commitOwner,
        { startupStage(KmHandshakeProbe.Stage.STATE_ACK); send(KmWire.payload("heartbeat")) },
    )
    val listeners = CopyOnWriteArraySet<(KmNativeStatus) -> Unit>()
    @Volatile var status = KmNativeStatus(); private set

    private val inventoryListener = object : android.hardware.input.InputManager.InputDeviceListener {
        override fun onInputDeviceAdded(deviceId: Int) = inventoryChanged()
        override fun onInputDeviceChanged(deviceId: Int) = inventoryChanged()
        override fun onInputDeviceRemoved(deviceId: Int) = inventoryChanged()
    }
    fun initialize(appContext: Context) {
        if (context != null) return
        context = appContext.applicationContext
        context!!.getSystemService(android.hardware.input.InputManager::class.java).registerInputDeviceListener(inventoryListener, worker)
    }
    private fun inventoryChanged() {
        if (KmKeyboardInventory.mustStop(status.state != "off", KmKeyBridgeLease.isEnabled(context!!),
                lease?.acknowledged == true, KmKeyboardInventory.anyPhysical())) {
            stopNow("물리 키 장치가 연결되어 KeyBridge 입력 소유권을 다시 확인해야 합니다")
        }
    }
    fun physicalKeyAllowed(): Boolean {
        val okay = lease?.acknowledged == true || context?.let { !KmKeyBridgeLease.isEnabled(it) } == true
        if (!okay && status.state != "off" && status.state != "cleanupFailed") stop("KeyBridge 입력 소유권 확인 전 물리 키 전달을 중지했습니다")
        return okay
    }
    fun changed() { listeners.forEach { it(status) } }
    private fun publish(value: KmNativeStatus) { status = value; main.post(::changed) }
    private fun startupStage(stage: KmHandshakeProbe.Stage) {
        if (handshakeProbe?.active == true) handshakeStage = stage
    }
    private fun startupFailure(admission: Long, error: Throwable) {
        if (observationSession.current(admission)) handshakeProbe?.record(handshakeStage, KmHandshakeProbe.Code.FAILED, error)
    }
    private fun task(action: () -> Unit) {
        val diagnosticAdmission = observationSession.ticket()
        if (pending.incrementAndGet() > 512) {
            pending.decrementAndGet(); stop("입력 처리 대기열이 초과되었습니다"); return
        }
        worker.post {
            pending.decrementAndGet()
            try { action() } catch (error: KmObservationUnavailable) {
                startupFailure(diagnosticAdmission, error)
                stopNow(KmObservationUnavailable.REASON)
            }
            catch (error: Unavailable) {
                startupFailure(diagnosticAdmission, error)
                stopNow(error.message)
            }
            catch (error: Exception) {
                startupFailure(diagnosticAdmission, error)
                stopNow("입력 권한 또는 입력 처리를 확인할 수 없습니다")
            }
        }
    }

    fun prepare(callback: (Result<KmNativeStatus>) -> Unit) {
        val admission = observationSession.ticket()
        task {
            if (!observationSession.current(admission)) {
                main.post { callback(Result.failure(IllegalStateException("입력 공유 준비가 취소되었습니다"))) }
                return@task
            }
            if (status.state != "off") { main.post { callback(Result.failure(IllegalStateException("기존 입력 공유를 먼저 중지하세요"))) }; return@task }
            preparation = callback
            handshakeProbe?.close()
            handshakeProbe = KmHandshakeProbe { observationSession.current(admission) }
            startupStage(KmHandshakeProbe.Stage.PREPARE)
            handshakeProbe?.record(handshakeStage, KmHandshakeProbe.Code.BEGIN)
            publish(KmNativeStatus("preparing"))
            val ctx = context ?: error("KM not initialized")
            if (KmAccessibilityService.instance == null) throw Unavailable("Leftcar 접근성 권한이 필요합니다")
            if (!Shizuku.pingBinder() || Shizuku.getUid() != 2000 || Shizuku.getVersion() < 13) throw Unavailable("USB로 Shizuku를 ADB 모드에서 시작하세요")
            if (Shizuku.checkSelfPermission() != PackageManager.PERMISSION_GRANTED) throw Unavailable("Shizuku 사용 권한이 필요합니다")
            val token = ++generation
            observationSessionTicket = admission
            val keyLease = KmKeyBridgeLease(ctx, worker) { stop("KeyBridge 입력 소유권 연결이 종료되었습니다") }
            lease = keyLease
            keyLease.acquire { okay ->
                if (generation != token || !observationSession.current(admission)) return@acquire
                if (!okay) { stopNow("KeyBridge 입력 공유 연동 API가 필요합니다. KeyBridge를 활성화한 채 지원 업데이트를 적용하세요"); return@acquire }
                bindAuthority(ctx, token, admission)
            }
            worker.postDelayed({ if (generation == token && authority == null) stopNow("Shizuku 입력 서비스 연결 시간이 초과되었습니다") }, 4000)
        }
    }

    private fun bindAuthority(ctx: Context, token: Long, admission: Long) {
        val probe = handshakeProbe
        val options = Shizuku.UserServiceArgs(ComponentName(ctx, KmInputAuthority::class.java))
            .processNameSuffix("km-authority").tag("leftcar-km-v1").version(2).daemon(false).debuggable(false)
        args = options
        val connection = object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, binder: IBinder?) = task {
                if (generation != token || !observationSession.current(admission)) return@task
                val api = IKmInputAuthority.Stub.asInterface(binder) ?: error("Missing KM authority")
                authority = api
                authorityBinder = binder
                deadAuthorityBinder = null
                binder!!.linkToDeath({
                    // A retained failed-cleanup authority can die after generation advances.
                    // Match this exact Binder on the worker; stale deaths cannot stop a replacement.
                    worker.post {
                        if (authorityBinder === binder) {
                            deadAuthorityBinder = binder
                            stopNow("Shizuku 입력 서비스가 종료되었습니다")
                        }
                    }
                }, 0)
                if (!api.prepare(lifetime)) throw Unavailable("UHID 또는 Android 16 마우스 관찰 기능을 사용할 수 없습니다")
                val service = KmAccessibilityService.instance ?: error("Accessibility disconnected")
                checkNoConflictingObserver(ctx, api)
                observationService = service
                service.configure(api, KmCaptureMode.OBSERVE,
                    freshGeneration = { generation == token && observationSession.current(admission) },
                    commitSession = { action -> observationSession.commit(admission, action) })
                val mouseCaptureSupported = api.mouseCaptureSupported()
                check(observationSession.commit(admission) {
                    preparedAt = SystemClock.elapsedRealtime()
                    publish(KmNativeStatus("preparing", null, true, true, mouseCaptureSupported, true))
                    main.post { if (observationSession.current(admission)) {
                        runCatching { ctx.startForegroundService(Intent(ctx, KmConnectionService::class.java)) }
                            .onFailure {
                                probe?.record(KmHandshakeProbe.Stage.PREPARE, KmHandshakeProbe.Code.FAILED, it)
                                probe?.finish(KmHandshakeProbe.Code.STOPPED)
                                stop("백그라운드 입력 서비스를 시작하지 못했습니다")
                            }
                    } }
                    val readyStatus = status
                    preparation?.let { ready -> main.post {
                        ready(if (observationSession.current(admission)) Result.success(readyStatus)
                            else Result.failure(IllegalStateException("입력 공유 준비가 취소되었습니다")))
                    } }; preparation = null
                })
                probe?.record(KmHandshakeProbe.Stage.PREPARE_READY, KmHandshakeProbe.Code.OK)
                heartbeat(token)
                flushLoop(token)
            }
            override fun onServiceDisconnected(name: ComponentName?) { if (generation == token) stop("Shizuku 연결이 종료되었습니다") }
            override fun onBindingDied(name: ComponentName?) { if (generation == token) stop("Shizuku 입력 서비스가 변경되었습니다") }
            override fun onNullBinding(name: ComponentName?) { if (generation == token) stop("Shizuku 입력 서비스를 연결할 수 없습니다") }
        }
        binding = connection
        main.post { runCatching { Shizuku.bindUserService(options, connection) }.onFailure {
            probe?.record(KmHandshakeProbe.Stage.PREPARE, KmHandshakeProbe.Code.FAILED, it)
            probe?.finish(KmHandshakeProbe.Code.STOPPED)
            stop("Shizuku 입력 서비스를 시작하지 못했습니다")
        } }
    }

    private fun checkNoConflictingObserver(ctx: Context, api: IKmInputAuthority) {
        check(!KmKeyboardInventory.mustStop(true, KmKeyBridgeLease.isEnabled(ctx), lease?.acknowledged == true, KmKeyboardInventory.anyPhysical())) {
            "KeyBridge physical key ownership unavailable"
        }
        ctx.getSystemService(AccessibilityManager::class.java)
            .getEnabledAccessibilityServiceList(AccessibilityServiceInfo.FEEDBACK_ALL_MASK).forEach { info ->
                val packageName = info.resolveInfo.serviceInfo.packageName
                if (packageName != ctx.packageName) {
                    check(api.observedSources(info) and (KmMotionSources.MOUSE_FAMILY and 255.inv()) == 0) { "다른 접근성 서비스가 마우스를 관찰 중입니다" }
                    check(packageName == KmKeyBridgeLease.PACKAGE || info.flags and AccessibilityServiceInfo.FLAG_REQUEST_FILTER_KEY_EVENTS == 0) { "다른 접근성 서비스가 키 입력을 필터링 중입니다" }
                }
            }
    }

    fun start(value: KmStartConfig, callback: (Result<KmNativeStatus>) -> Unit) = worker.post {
        if (starting.pending) {
            main.post { callback(Result.failure(IllegalStateException("입력 공유 연결을 이미 시작하고 있습니다"))) }
            return@post
        }
        startTicket = starting.begin { result -> main.post { callback(result) } }
        val probe = handshakeProbe
        try {
            startupStage(KmHandshakeProbe.Stage.START_CONFIG)
            probe?.record(handshakeStage, KmHandshakeProbe.Code.BEGIN)
            value.validate()
            startupStage(KmHandshakeProbe.Stage.START_PRECONDITION)
            check(status.state == "preparing" && status.mouseObservation && authority != null && transport == null)
            check(!value.androidToMac || status.mouseCapture)
            startupStage(KmHandshakeProbe.Stage.START_SECRET)
            val secret = Base64.decode(value.sessionKey, Base64.URL_SAFE or Base64.NO_WRAP or Base64.NO_PADDING)
            require(secret.size == 32)
            config = value; gate = KmEpochGate(value.epoch); outgoingSeq = 0
            lastReceived = SystemClock.elapsedRealtime()
            val token = generation
            try {
                startupStage(KmHandshakeProbe.Stage.TRANSPORT_SETUP)
                transport = KmDuplex(value.host, value.port, secret, value.sessionId,
                    { incoming -> task { if (generation == token) receive(incoming) } },
                    { reason -> if (generation == token) stop(reason) }, probe)
            } finally { secret.fill(0) }
            startupStage(KmHandshakeProbe.Stage.HELLO_QUEUE)
            probe?.record(handshakeStage, KmHandshakeProbe.Code.BEGIN)
            send(KmWire.payload("hello"))
            startupStage(KmHandshakeProbe.Stage.STATE_WAIT)
            // Hello is not readiness. The receive path settles only after State applies.
            worker.postDelayed({
                if (generation == token && starting.pending) {
                    probe?.record(KmHandshakeProbe.Stage.STATE_WAIT, KmHandshakeProbe.Code.FAILED)
                    stopNow("입력 공유 상태 확인 시간이 초과되었습니다")
                }
            }, 1000)
        } catch (error: Exception) {
            probe?.record(handshakeStage, KmHandshakeProbe.Code.FAILED, error)
            stopNow("입력 공유 연결을 시작할 수 없습니다")
        }
    }

    private fun heartbeat(token: Long) {
        if (token != generation || authority == null) return
        try {
            authority!!.heartbeat()
            check(KmAccessibilityService.instance != null)
            if (transport != null) {
                check(SystemClock.elapsedRealtime() - lastReceived <= 1000)
                if (!transition.waitingForPlacement) send(KmWire.payload("heartbeat"))
            } else check(SystemClock.elapsedRealtime() - preparedAt < 30000)
            if (placement != null) check(SystemClock.elapsedRealtime() - placementStarted <= 1000)
            worker.postDelayed({ heartbeat(token) }, 100)
        } catch (error: Exception) {
            handshakeProbe?.record(KmHandshakeProbe.Stage.HEARTBEAT, KmHandshakeProbe.Code.FAILED, error)
            stopNow("입력 공유 응답 또는 권한이 만료되었습니다")
        }
    }

    private fun flushLoop(token: Long) {
        if (token != generation || authority == null) return
        runCatching { flushMotion() }.onFailure {
            handshakeProbe?.record(KmHandshakeProbe.Stage.FLUSH, KmHandshakeProbe.Code.FAILED, it)
            stopNow("입력 전송 대기열이 초과되었습니다")
        }
        worker.postDelayed({ flushLoop(token) }, 8)
    }

    private fun receive(message: KmWire) {
        startupStage(KmHandshakeProbe.Stage.STATE_GATE)
        val currentGate = gate ?: error("No KM epoch")
        currentGate.accept(message.epoch, message.seq, message.kind)
        lastReceived = SystemClock.elapsedRealtime()
        when (message.kind) {
            "state" -> {
                handshakeProbe?.record(KmHandshakeProbe.Stage.STATE_APPLY, KmHandshakeProbe.Code.BEGIN)
                startupStage(KmHandshakeProbe.Stage.STATE_OWNER_GUARD)
                changeOwner(message.payload.getString("owner"))
                startupStage(KmHandshakeProbe.Stage.STATE_COMMIT)
                check(observationSession.commit(observationSessionTicket) {
                    if (status.state in setOf("armed", "macToAndroid", "androidToMac")) starting.applied(startTicket, status)
                })
                if (!starting.pending) handshakeProbe?.finish(KmHandshakeProbe.Code.APPLIED)
            }
            "heartbeat", "hello" -> Unit
            "stop" -> stopNow("Mac에서 입력 공유를 중지했습니다")
            "releaseAll" -> {
                authority!!.releaseAll(); delayedInput.clear(); motionX = 0.0; motionY = 0.0
                remoteReleasePending = owner == "androidToMac"
                returnPlacementReceived = false
            }
            "cursor" -> {
                check(owner == "macToAndroid" || owner == "androidToMac" && remoteReleasePending)
                if (owner == "androidToMac") returnPlacementReceived = true
                val bounds = context!!.getSystemService(android.view.WindowManager::class.java).maximumWindowMetrics.bounds
                val width = bounds.width(); val height = bounds.height()
                val tx = message.payload.getDouble("x") / message.payload.getInt("width") * width
                val ty = message.payload.getDouble("y") / message.payload.getInt("height") * height
                placementStarted = SystemClock.elapsedRealtime()
                placement = KmCursorPlacement(tx.coerceIn(0.0, width - 1.0), ty.coerceIn(0.0, height - 1.0), placementStarted)
                // Get a current OS position even if the virtual mouse has not moved yet.
                authority!!.inject(KmInputAuthority.MOVE, 0, false, 1.0, 0.0)
            }
            "move", "button", "wheel", "key" -> {
                check(owner == "macToAndroid" && config!!.macToAndroid)
                if (placement != null) { check(delayedInput.size < 128); delayedInput.addLast(message.payload) }
                else inject(message.payload)
            }
            else -> error("Unexpected host KM message")
        }
    }

    private fun changeOwner(next: String) {
        if (owner == "androidToMac" && next == "local") check(remoteReleasePending && returnPlacementReceived)
        transport?.discardQueued()
        transition.apply(next, placement != null)
    }

    private fun commitOwner(next: String) {
        startupStage(KmHandshakeProbe.Stage.STATE_CONTEXT)
        val api = authority ?: error("No authority")
        val service = KmAccessibilityService.instance ?: error("No accessibility service")
        motionX = 0.0; motionY = 0.0; sentButtons = 0; delayedInput.clear(); placement = null
        edgePending = false; returnArmed = false; remoteReleasePending = false; returnPlacementReceived = false
        startupStage(KmHandshakeProbe.Stage.STATE_COEXISTENCE)
        checkNoConflictingObserver(context!!, api)
        when (next) {
            "androidToMac" -> {
                startupStage(KmHandshakeProbe.Stage.STATE_CAPTURE)
                check(config!!.androidToMac)
                val ticket = service.beginCapture()
                val mouse = service.physicalMouseName ?: error("No physical Android mouse")
                check(api.captureMouse(mouse, rawMouseCallback)) { "물리 마우스 전역 캡처를 사용할 수 없습니다" }
                startupStage(KmHandshakeProbe.Stage.STATE_CONFIGURE)
                service.configure(api, KmCaptureMode.CONSUME, ticket, freshGeneration = observationFence(service), commitSession = observationCommit()) { owner = next }
            }
            "macToAndroid" -> {
                startupStage(KmHandshakeProbe.Stage.STATE_CONFIGURE)
                check(config!!.macToAndroid); service.configure(api, KmCaptureMode.OBSERVE, freshGeneration = observationFence(service), commitSession = observationCommit()) { owner = next }
            }
            "local" -> {
                startupStage(KmHandshakeProbe.Stage.STATE_CONFIGURE)
                service.configure(api, KmCaptureMode.OBSERVE, freshGeneration = observationFence(service), commitSession = observationCommit()) { owner = next }
            }
            else -> error("Unknown owner")
        }
        startupStage(KmHandshakeProbe.Stage.STATE_COMMIT)
        check(observationSession.commit(observationSessionTicket) {
            publish(status.copy(state = if (next == "local") "armed" else next, reason = null))
        })
    }

    private fun inject(p: JSONObject) {
        val api = authority ?: error("No input authority")
        when (p.getString("kind")) {
            "move" -> api.inject(KmInputAuthority.MOVE, 0, false, p.getDouble("dx"), p.getDouble("dy"))
            "wheel" -> api.inject(KmInputAuthority.WHEEL, 0, false, p.getDouble("dx"), p.getDouble("dy"))
            "button" -> api.inject(KmInputAuthority.BUTTON, p.getInt("button"), p.getBoolean("down"), 0.0, 0.0)
            "key" -> api.inject(KmInputAuthority.KEY, p.getInt("code"), p.getBoolean("down"), 0.0, 0.0)
        }
    }

    fun motion(pointer: KmObservedPointer) = task {
        lastCursor = pointer
        if (transport == null || status.state == "preparing") return@task
        if (pointer.own && owner != "macToAndroid" && placement == null) return@task
        val target = placement
        if (pointer.own && target != null) {
            val step = target.observe(pointer.x, pointer.y, SystemClock.elapsedRealtime())
            if (step == null) {
                placement = null
                transition.placementCompleted()
                while (delayedInput.isNotEmpty()) inject(delayedInput.removeFirst())
            } else {
                authority!!.inject(KmInputAuthority.MOVE, 0, false, step.first, step.second)
            }
        }
        if (remoteReleasePending || transition.waitingForPlacement) return@task
        val now = SystemClock.elapsedRealtime()
        val side = config!!.arrangementSide
        val distance = if (side == "right") pointer.x else pointer.width - 1 - pointer.x
        if (distance >= 8) returnArmed = true
        val canCross = !edgePending && placement == null && distance <= 1 && returnArmed &&
            (pointer.own && owner == "macToAndroid" || pointer.physical && owner == "local" && !KmAccessibilityService.instance!!.hasLocalHolds())
        if (now - lastCursorSent >= 8 || canCross) {
            send(KmWire.payload("cursor", "x" to pointer.x.coerceIn(0.0, pointer.width.toDouble()),
                "y" to pointer.y.coerceIn(0.0, pointer.height.toDouble()), "width" to pointer.width, "height" to pointer.height))
            lastCursorSent = now
        }
        if (canCross && (owner == "macToAndroid" || config!!.androidToMac)) {
            edgePending = true
            send(KmWire.payload("edge", "returning" to (owner == "macToAndroid")))
        }
    }

    fun key(usage: Int, down: Boolean) = task {
        if (owner == "androidToMac" && transport != null && !remoteReleasePending) {
            flushMotion(); send(KmWire.payload("key", "code" to usage, "down" to down))
        }
    }

    private val rawMouseCallback = object : IKmMouseCallback.Stub() {
        override fun onMouse(dx: Double, dy: Double, wheelX: Double, wheelY: Double, buttons: Int) = task {
            if (owner != "androidToMac" || transport == null || remoteReleasePending) return@task
            motionX += dx; motionY += dy
            check(abs(motionX) <= 65536 && abs(motionY) <= 65536)
            if (wheelX != 0.0 || wheelY != 0.0 || buttons != sentButtons) flushMotion()
            if (wheelX != 0.0 || wheelY != 0.0) send(KmWire.payload("wheel", "dx" to wheelX, "dy" to wheelY))
            for (button in 0..4) if ((buttons xor sentButtons) and (1 shl button) != 0) {
                send(KmWire.payload("button", "button" to button, "down" to (buttons and (1 shl button) != 0)))
            }
            sentButtons = buttons
        }
        override fun onCaptureLost() { stop("물리 마우스 캡처가 종료되었습니다") }
    }

    private fun flushMotion() {
        if (motionX == 0.0 && motionY == 0.0) return
        send(KmWire.payload("move", "dx" to motionX, "dy" to motionY))
        motionX = 0.0; motionY = 0.0
    }
    private fun send(payload: JSONObject) {
        val current = config ?: return
        check(outgoingSeq < Long.MAX_VALUE)
        transport?.send(KmWire.encode(current.sessionId, gate!!.epoch, ++outgoingSeq, payload))
    }

    private fun observationFence(service: KmAccessibilityService): () -> Boolean {
        val token = generation
        val admission = observationSessionTicket
        return { generation == token && observationSession.current(admission) && KmAccessibilityService.instance === service }
    }
    private fun observationCommit(): (() -> Unit) -> Boolean {
        val admission = observationSessionTicket
        return { action -> observationSession.commit(admission, action) }
    }

    fun stop(reason: String? = null, completion: ((Result<Unit>) -> Unit)? = null) {
        observationSession.cancel() // Synchronous admission/commit fence, independent of worker progress.
        (observationService ?: KmAccessibilityService.instance)?.invalidateObservation()
        worker.post { val result = stopNow(reason); if (completion != null) main.post { completion(result) } }
    }
    private fun stopNow(reason: String?): Result<Unit> {
        handshakeProbe?.finish(KmHandshakeProbe.Code.STOPPED)
        observationSession.cancel()
        (observationService ?: KmAccessibilityService.instance)?.invalidateObservation()
        generation++
        runCatching { if (transport != null) send(KmWire.payload("stop")) }
        transport?.close(); transport = null
        val api = authority
        val capturedBinder = authorityBinder
        var localRestored = false
        val cleanup = KmOwnerTransition(
            { api?.releaseAll() }, { api?.releaseMouse() },
            { val service = observationService ?: KmAccessibilityService.instance
                localRestored = service?.restoreLocal() ?: (observationService == null)
                check(localRestored) }, {},
        )
        val allCleaned = cleanup.disconnect { api?.close() }
        // Only this helper's death recipient or Binder liveness proves its process is gone.
        // RPC exceptions alone never prove FD closure. Local observation must still be restored.
        val helperDead = capturedBinder != null && (deadAuthorityBinder === capturedBinder ||
            runCatching { !capturedBinder.isBinderAlive }.getOrDefault(false))
        val cleanupFailed = !allCleaned && !(helperDead && localRestored)
        transition.cancel()
        lease?.close(); lease = null
        if (!cleanupFailed) {
            observationService = null
            authority = null; authorityBinder = null; deadAuthorityBinder = null
            val oldBinding = binding; val oldArgs = args; binding = null; args = null
            if (oldBinding != null && oldArgs != null) main.post { runCatching { Shizuku.unbindUserService(oldArgs, oldBinding, true) } }
        } // Retain failed cleanup authority for explicit retry; watchdog remains a fallback.

        config = null; gate = null; owner = "local"; placement = null; delayedInput.clear(); remoteReleasePending = false; returnPlacementReceived = false
        motionX = 0.0; motionY = 0.0; edgePending = false; returnArmed = false; sentButtons = 0
        val failure = if (cleanupFailed) "입력 서비스 해제를 확인하지 못했습니다. Leftcar 접근성을 직접 끄세요. helper는 heartbeat 만료 시 1초 내 입력을 해제합니다" else reason
        publish(KmNativeStatus(state = if (cleanupFailed) "cleanupFailed" else "off", reason = failure))
        starting.failed(failure ?: "입력 공유 시작이 취소되었습니다")
        preparation?.let { completion -> main.post { completion(Result.failure(IllegalStateException(failure ?: "입력 공유 준비가 취소되었습니다"))) } }; preparation = null
        if (!cleanupFailed) context?.let { ctx -> main.post { ctx.stopService(Intent(ctx, KmConnectionService::class.java)) } }
        return if (cleanupFailed) Result.failure(IllegalStateException(failure)) else Result.success(Unit)
    }
}
