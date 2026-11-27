import { useParams } from "volt-rs/navigation";

// Shares its pattern with @side/[id]: the page renders as children, not shop/default.tsx.
export default function Product() {
  const { slug } = useParams<{ slug: string }>();
  return <h1 id="product">Product {slug}</h1>;
}
