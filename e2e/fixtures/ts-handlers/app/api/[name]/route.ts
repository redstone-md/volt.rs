export async function GET(_request: Request, { params }: { params: Promise<{ name: string }> }) {
  return Response.json({ from: "typescript", name: (await params).name });
}
