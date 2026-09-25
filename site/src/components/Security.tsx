import { SITE_CONTENT } from '../content/site';
import './Security.css';

export function Security() {
  const { security } = SITE_CONTENT;

  return (
    <section id="security" className="security-band">
      <div className="section-container">
        <h2 className="section-title">보안</h2>
        <p className="section-lead">{security.lead}</p>
        <ul className="security-list">
          {security.items.map((item) => (
            <li key={item.id} className="security-row">
              <h3 className="security-row-title">{item.title}</h3>
              <p className="security-row-desc">{item.description}</p>
            </li>
          ))}
        </ul>
      </div>
    </section>
  );
}
