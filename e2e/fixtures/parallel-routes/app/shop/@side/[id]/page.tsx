import { useParams } from "volt-rs/navigation";

export default function SideItem() {
  const { id } = useParams<{ id: string }>();
  return <p id="shop-side-item">Side for {id}</p>;
}
