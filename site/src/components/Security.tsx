import { SITE_CONTENT } from '../content/site';
import { Card } from './Card';
import './Security.css';

export function Security() {
  const { security } = SITE_CONTENT;

  return (
    <section id="security" className="section-container">
      <h2 className="section-title">Security</h2>
      <p className="section-lead">{security.lead}</p>
      <div className="security-grid">
        {security.items.map((item) => (
          <Card key={item.id} title={item.title}>
            {item.description}
          </Card>
        ))}
      </div>
    </section>
  );
}
