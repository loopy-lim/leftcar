import { SITE_CONTENT } from '../content/site';
import './Status.css';

export function Status() {
  const { status } = SITE_CONTENT;

  return (
    <section id="status" className="section-container status-section">
      <h2 className="section-title">지원 현황</h2>
      <p className="section-lead">{status.lead}</p>
      <dl className="status-list">
        {status.items.map((item) => (
          <div key={item.id} className="status-row">
            <dt className="status-term">{item.title}</dt>
            <dd className={item.highlight ? 'status-highlight' : 'status-normal'}>
              {item.status}
            </dd>
          </div>
        ))}
      </dl>
      <p className="status-note">{status.note}</p>
    </section>
  );
}
