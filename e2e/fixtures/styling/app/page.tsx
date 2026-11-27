import styles from "./page.module.css";
import card from "./card.module.scss";

export default function Home() {
  return (
    <main>
      <h1 id="title" className={styles.title}>
        Styled
      </h1>
      <p id="global" className="title">
        Unscoped
      </p>
      <div id="card" className={card.card}>
        Card
      </div>
      <div id="dotted" className="dotted">
        <span className="inner">Dotted</span>
      </div>
      <i id="icon" className={styles.icon} />
      <code id="mono" className="mono">
        mono
      </code>
    </main>
  );
}
