import styles from "./page.module.css";
import card from "./card.module.scss";
// Imported for its CSS only: no class is referenced.
import side from "./side.module.css";

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
      <p id="plain" className="plainimported">
        Plain
      </p>
      <i id="partbg" className="partbg" />
      <p id="fromimport" className="fromimport">
        Imported
      </p>
      <p id="remote" className="remote">
        Remote
      </p>
      <code id="mono" className="mono">
        mono
      </code>
    </main>
  );
}
