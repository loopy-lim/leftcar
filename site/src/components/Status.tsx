import { SITE_CONTENT } from '../content/site';
import { Card } from './Card';
import './Status.css';

export function Status() {
  const { status } = SITE_CONTENT;

  return (
    <section id="status" className="section-container">
      <h2 className="section-title">Status</h2>
      <p className="section-lead">{status.lead}</p>
      
      <div className="status-grid">
        {status.items.map((item) => (
          <Card key={item.id} title={item.title}>
            <span className={item.highlight ? 'status-highlight' : 'status-normal'}>
              {item.status}
            </span>
          </Card>
        ))}
      </div>
      
      <p className="status-note">{status.note}</p>
    </section>
  );
}
