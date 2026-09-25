import { SITE_CONTENT } from '../content/site';
import { Card } from './Card';
import './FeatureGrid.css';

export function FeatureGrid() {
  const { features, featuresLead } = SITE_CONTENT;

  return (
    <section id="features" className="section-container">
      <h2 className="section-title">주요 기능</h2>
      <p className="section-lead">{featuresLead}</p>
      <div className="features-grid">
        {features.map((feature) => (
          <Card key={feature.id} title={feature.title}>
            {feature.description}
          </Card>
        ))}
      </div>
    </section>
  );
}
