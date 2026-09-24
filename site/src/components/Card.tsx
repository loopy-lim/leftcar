import './Card.css';

interface CardProps {
  title: string;
  children: React.ReactNode;
}

export function Card({ title, children }: CardProps) {
  return (
    <article className="card">
      <h3 className="card-title">{title}</h3>
      <p className="card-desc">{children}</p>
    </article>
  );
}
