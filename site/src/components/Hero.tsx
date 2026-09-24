import { SITE_CONTENT } from '../content/site';
import { Diagram } from './Diagram';
import './Hero.css';

export function Hero() {
  const { hero } = SITE_CONTENT;

  return (
    <header className="hero" id="hero">
      <div className="hero-badges">
        <span className="badge">{hero.badge}</span>
        <span className="badge version-badge">{hero.versionBadge}</span>
      </div>
      <h1 className="hero-headline">{hero.headline}</h1>
      <p className="hero-subcopy">{hero.subcopy}</p>
      <div className="hero-ctas">
        <a href={hero.cta1.url} className="cta primary-cta">{hero.cta1.label}</a>
        <a href={hero.cta2.url} className="cta secondary-cta">{hero.cta2.label}</a>
      </div>
      <Diagram />
    </header>
  );
}
