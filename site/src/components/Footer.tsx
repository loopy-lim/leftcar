import { SITE_CONTENT } from '../content/site';
import './Footer.css';

export function Footer() {
  const { footer } = SITE_CONTENT;

  return (
    <footer id="footer" className="footer">
      <div className="footer-container">
        <ul className="footer-links">
          {footer.links.map((link) => (
            <li key={link.id}>
              <a href={link.url}>{link.label}</a>
            </li>
          ))}
        </ul>
        <p className="footer-copyright">{footer.copyright}</p>
      </div>
    </footer>
  );
}
