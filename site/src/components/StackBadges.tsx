import { SITE_CONTENT } from '../content/site';
import './StackBadges.css';

export function StackBadges() {
  const { stack } = SITE_CONTENT;

  return (
    <section id="stack" className="section-container stack-section">
      <p className="stack-label">기술 스택</p>
      <p className="stack-line">{stack.badges.join(', ')}</p>
      <p className="stack-desc">{stack.description}</p>
    </section>
  );
}
