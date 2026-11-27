export default function FeedItem({ params }: { params: { id: string } }) {
  return <h1 id="item">Feed preview of item {params.id}</h1>;
}
