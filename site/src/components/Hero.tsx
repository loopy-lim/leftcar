import { SITE_CONTENT } from '../content/site';
import './Hero.css';

export function Hero() {
  const { hero } = SITE_CONTENT;

  return (
    <header className="hero" id="hero">
      <p className="hero-meta">{hero.badge}</p>
      <h1 className="hero-headline">{hero.headline}</h1>
      <p className="hero-subcopy">{hero.subcopy}</p>
      <div className="hero-ctas">
        <a href={hero.cta1.url} className="cta primary-cta">{hero.cta1.label}</a>
        <a href={hero.cta2.url} className="cta secondary-cta">{hero.cta2.label}</a>
      </div>
    </header>
  );
}
