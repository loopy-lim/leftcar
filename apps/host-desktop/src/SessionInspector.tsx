import type { SessionRow } from "./sessionTypes";
import { encoderDiagnosticsView } from "./encoderDiagnostics";
import { cn } from "./lib/cn";
import { inspectorButtonVariants } from "./lib/variants";

interface SessionInspectorProps {
  session: SessionRow;
  transportLabel: string;
  qualitySupported: boolean;
  qualityPercent: number;
  qualityBusy: boolean;
  onSetQuality: (session: SessionRow, quality: number | null) => Promise<void>;
}

export default function SessionInspector({
  session,
  transportLabel,
  qualitySupported,
  qualityPercent,
  qualityBusy,
  onSetQuality,
}: SessionInspectorProps) {
  const encoderDiagnostics = encoderDiagnosticsView(session);

  return (
    <div className="inspector-panel">
      <span className="inspector-header">연결 상세 정보</span>
      <div className="inspector-grid">
        <div className="inspector-item">
          <span className="inspector-item-label">단계별 FPS (캡처 / 제출 / 출력)</span>
          <span className="inspector-item-value">
            {session.captureFps ?? "측정 중"}
            {" / "}
            {session.encodeSubmitFps ?? session.fps}
            {" / "}
            {session.encodeOutputFps ?? "측정 중"}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">실제 Android 렌더 FPS</span>
          <span className="inspector-item-value">
            {session.renderedFps != null ? `${session.renderedFps} FPS` : "feedback 대기 중"}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">인코더 제출 실패 / in-flight</span>
          <span className="inspector-item-value">
            {session.encodeSubmitFailures ?? 0}
            {" / "}
            {session.encodeInFlight ?? 0}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">인코더 출력 간격 p95</span>
          <span className="inspector-item-value">
            {session.encodeOutputIntervalP95Us != null
              ? `${(session.encodeOutputIntervalP95Us / 1000).toFixed(1)}ms`
              : "측정 중"}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">화면 가져오기</span>
          <span className="inspector-item-value">
            {session.captureToEncodeUs !== undefined
              ? `${(session.captureToEncodeUs / 1000).toFixed(1)}ms`
              : "<2ms"}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">처리 대기</span>
          <span className="inspector-item-value">
            {session.captureQueueWaitUs !== undefined
              ? `${(session.captureQueueWaitUs / 1000).toFixed(1)}ms`
              : "0.1ms"}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">영상 처리</span>
          <span className="inspector-item-value">
            {session.encodeOutputUs !== undefined
              ? `${(session.encodeOutputUs / 1000).toFixed(1)}ms`
              : "<2ms"}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">인코더 출력 / 패킷화</span>
          <span className="inspector-item-value">
            {session.encodeOutputP95Us != null
              ? `${(session.encodeOutputP95Us / 1000).toFixed(1)}ms`
              : "측정 중"}
            {" / "}
            {session.packetizationP95Us != null
              ? `${(session.packetizationP95Us / 1000).toFixed(1)}ms`
              : "측정 중"}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">네트워크 전송</span>
          <span className="inspector-item-value">
            {session.sendBlockUs !== undefined
              ? `${(session.sendBlockUs / 1000).toFixed(1)}ms`
              : "0.2ms"}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">실제 전송 경로</span>
          <span className="inspector-item-value">{transportLabel}</span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">수신 RTT / 디코더</span>
          <span className="inspector-item-value">
            {session.receiverRttMs != null ? `${session.receiverRttMs}ms` : "측정 중"}
            {" / "}
            {session.receiverWireMs != null ? `${session.receiverWireMs}ms` : "측정 중"}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">수신 손실 / feedback</span>
          <span className="inspector-item-value">
            {(session.receiverFrameGaps ?? 0) + (session.receiverIncompleteAus ?? 0)}
            {" / "}
            {session.receiverFeedbackAgeMs != null
              ? `${session.receiverFeedbackAgeMs}ms 전`
              : "대기 중"}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">Host 큐 드롭</span>
          <span className="inspector-item-value">
            일반 {Math.max(0, (session.networkQueueDropped ?? 0) - (session.recoveryFramesDropped ?? 0))}
            {" / 복구 "}
            {session.recoveryFramesDropped ?? 0}
            {" / 캡처 "}
            {session.captureQueueDropped ?? 0}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">Host 큐 점유 / oldest</span>
          <span className="inspector-item-value">
            {session.pendingFrameBytes ?? 0}B
            {" / "}
            {((session.pendingFrameOldestAgeUs ?? 0) / 1000).toFixed(1)}ms
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">최근 AU burst</span>
          <span className="inspector-item-value">
            {session.lastAuBytes !== undefined
              ? `${(session.lastAuBytes / 1024).toFixed(0)}KB · ${session.lastAuFragments ?? 0} + ${session.lastAuParity ?? 0}개 · ${((session.lastAuSendUs ?? 0) / 1000).toFixed(1)}ms`
              : "측정 중"}
            {session.lastAuIsKeyframe ? " · IDR" : ""}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">UDP datagram</span>
          <span className="inspector-item-value">
            {session.sentDatagrams ?? 0}개 전송 · 실패 {session.udpSendFailures ?? 0}
            {" · parity "}
            {session.sentParityDatagrams ?? 0}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">복구 요청 / IDR</span>
          <span className="inspector-item-value">
            {session.recoveryRequestsSuppressed ?? 0} 억제 / {session.recoveryKeyframes ?? 0}회
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">느린 경우 화면 처리 / 전송</span>
          <span className="inspector-item-value">
            {session.captureToEncodeP95Us
              ? `${(session.captureToEncodeP95Us / 1000).toFixed(1)}ms`
              : "1.2ms"} / {session.sendBlockP95Us ? `${(session.sendBlockP95Us / 1000).toFixed(1)}ms` : "0.5ms"}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">UDP pacing p95</span>
          <span className="inspector-item-value">
            {session.sendPaceP95Us !== undefined
              ? `${(session.sendPaceP95Us / 1000).toFixed(1)}ms`
              : "측정 중"}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">현재 인코더 목표</span>
          <span className="inspector-item-value">
            {session.currentBitrate !== undefined
              ? `${(session.currentBitrate / 1_000_000).toFixed(1)}Mbps`
              : "측정 중"}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">인코더 경로</span>
          <span className="inspector-item-value">
            {encoderDiagnostics.path} · {encoderDiagnostics.identity}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">인코더 설정</span>
          <span className="inspector-item-value">
            {encoderDiagnostics.configuration} · 적용 {encoderDiagnostics.applied}
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">미지원/거부 속성</span>
          <span className="inspector-item-value">{encoderDiagnostics.unavailable}</span>
        </div>
        {encoderDiagnostics.fallback !== null ? (
          <div className="inspector-item">
            <span className="inspector-item-label">인코더 fallback</span>
            <span className="inspector-item-value">{encoderDiagnostics.fallback}</span>
          </div>
        ) : null}
        <div className="inspector-item">
          <span className="inspector-item-label">동적 화질 힌트</span>
          <span className="inspector-item-value">
            {session.qualityHint != null
              ? `${session.qualityHint.toFixed(2)} · ${session.qualityAdaptationChanges ?? 0}회 변경 / ${session.qualityAdaptationChecks ?? 0}회 확인 · ${session.qualityAdaptationLastStatus ?? "대기"}`
              : "적용 안 됨"}
          </span>
        </div>
        <div className="inspector-item quality-override-item">
          <div className="quality-override-heading">
            <span className="inspector-item-label">수동 화질 상한</span>
            <span className="inspector-item-value">
              {session.qualityOverride != null ? `${qualityPercent}% 고정` : "자동"}
            </span>
          </div>
          <div className="quality-override-controls">
            <span className="quality-override-endpoint">낮음</span>
            <input
              key={`${session.session}-${session.qualityOverride ?? "auto"}-${Math.round((session.qualityHint ?? 0.5) * 100)}`}
              type="range"
              min="25"
              max="50"
              step="5"
              defaultValue={qualityPercent}
              disabled={!qualitySupported || session.state !== "running" || qualityBusy}
              aria-label="수동 화질 상한"
              onChange={(event) => {
                void onSetQuality(session, Number(event.currentTarget.value) / 100);
              }}
            />
            <span className="quality-override-endpoint">기본</span>
            <button
              className={cn(
                "btn-ghost btn-sm quality-auto-button",
                inspectorButtonVariants(),
              )}
              disabled={!qualitySupported || session.qualityOverride == null || qualityBusy}
              onClick={() => void onSetQuality(session, null)}
            >
              자동 복귀
            </button>
          </div>
          <span className="quality-override-help">
            고변화 장면에서 프레임을 지키려면 낮추고, 여유가 생기면 자동 복귀하세요.
          </span>
        </div>
        <div className="inspector-item">
          <span className="inspector-item-label">화면 처리 방식</span>
          <span className="inspector-item-value" style={{ textTransform: "capitalize" }}>
            {session.captureBackend || "ScreenCaptureKit"}
          </span>
        </div>
      </div>
    </div>
  );
}
