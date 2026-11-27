export async function GET(_request: Request, { params }: { params: Promise<{ id: string }> }) {
  const { id } = await params;
  return Response.json({ id, double: Number(id) * 2 });
}

export function DELETE() {
  return new Response(null, { status: 204, headers: { "x-deleted": "1" } });
}
