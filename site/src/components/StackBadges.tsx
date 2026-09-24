import { SITE_CONTENT } from '../content/site';
import './StackBadges.css';

export function StackBadges() {
  const { stack } = SITE_CONTENT;

  return (
    <section id="stack" className="section-container stack-section">
      <h2 className="section-title">Tech Stack</h2>
      <div className="stack-badges">
        {stack.badges.map((badge) => (
          <span key={badge} className="stack-badge">{badge}</span>
        ))}
      </div>
      <p className="stack-desc">{stack.description}</p>
    </section>
  );
}
