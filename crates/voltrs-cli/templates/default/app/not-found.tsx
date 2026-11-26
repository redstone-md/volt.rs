import Link from "volt-rs/link";

import { Button } from "@/components/ui/button";

export const metadata = { title: "Not found", robots: { index: false } };

export default function NotFound() {
  return (
    <div className="space-y-4 text-center">
      <h1 className="text-3xl font-bold">404</h1>
      <p className="text-muted-foreground">This page could not be found.</p>
      <Button asChild variant="outline">
        <Link href="/">Go home</Link>
      </Button>
    </div>
  );
}
