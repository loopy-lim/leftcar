import { SITE_CONTENT } from '../content/site';
import { Card } from './Card';
import './FeatureGrid.css';

export function FeatureGrid() {
  const { features } = SITE_CONTENT;

  return (
    <section id="features" className="section-container">
      <h2 className="sr-only">Features</h2>
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
