import './Diagram.css';

export function Diagram() {
  return (
    <figure className="diagram-container" aria-label="PC에서 Android로 여러 독립 창을 전송하는 모식도">
      <div className="diagram-pc" aria-hidden="true">
        <div className="pc-screen"></div>
        <div className="pc-screen offset"></div>
      </div>
      <div className="diagram-network" aria-hidden="true">
        <div className="network-line"></div>
        <span className="network-label">로컬 네트워크 · 암호화</span>
        <div className="network-arrow">→</div>
      </div>
      <div className="diagram-android" aria-hidden="true">
        <div className="android-window">
          <div className="window-header"></div>
          <div className="window-body"></div>
        </div>
        <div className="android-window">
          <div className="window-header"></div>
          <div className="window-body"></div>
        </div>
        <div className="android-window">
          <div className="window-header"></div>
          <div className="window-body"></div>
        </div>
      </div>
    </figure>
  );
}
