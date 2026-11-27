import { useState } from "react";
import Link from "volt-rs/link";

export const metadata = { title: "Photos" };

export default function Photos() {
  // Local state shows whether the gallery stays mounted behind the modal.
  const [likes, setLikes] = useState(0);
  return (
    <main>
      <h1>Photos</h1>
      <button type="button" id="like" onClick={() => setLikes(likes + 1)}>
        Likes {likes}
      </button>
      <ul>
        {[1, 2, 3, 4, 5, 6].map((id) => (
          <li key={id}>
            <Link href={`/photos/${id}`} id={`photo-${id}`}>
              Photo {id}
            </Link>
          </li>
        ))}
      </ul>
    </main>
  );
}
