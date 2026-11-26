import { addTodo, toggleTodo, type PageProps } from "@volt/api";
import Form, { useFormPending } from "volt-rs/form";
import { Check, Plus } from "lucide-react";

import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { cn } from "@/lib/utils";

export const metadata = { title: "Todos" };

function AddButton() {
  const pending = useFormPending();
  return (
    <Button type="submit" disabled={pending}>
      <Plus /> Add
    </Button>
  );
}

export default function Home({ data }: PageProps<"/">) {
  return (
    <div className="space-y-8">
      <section className="space-y-2">
        <h1 className="text-3xl font-bold tracking-tight">React on a Rust server</h1>
        <p className="text-muted-foreground">
          The list below came from a Rust loader in {data.loadedInMicros}µs and was rendered to HTML by React inside
          the Rust process. Every change runs a Rust server action, and the forms keep working with JavaScript
          disabled.
        </p>
      </section>

      <Card>
        <CardHeader>
          <CardTitle>Todos</CardTitle>
          <CardDescription>
            {data.todos.filter((t) => t.done).length} of {data.todos.length} done
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          <Form action={addTodo} className="flex gap-2" onSuccess={(_, form) => form.reset()}>
            <Input name="title" placeholder="What needs doing?" required maxLength={120} />
            <AddButton />
          </Form>
          <ul className="divide-y rounded-md border">
            {data.todos.map((todo) => (
              <li key={todo.id}>
                <Form action={toggleTodo} className="flex items-center gap-3 px-3 py-2">
                  <input type="hidden" name="id" value={todo.id} />
                  <button
                    type="submit"
                    aria-label={todo.done ? "Mark as not done" : "Mark as done"}
                    className={cn(
                      "flex size-5 items-center justify-center rounded border",
                      todo.done && "bg-primary text-primary-foreground",
                    )}
                  >
                    {todo.done && <Check className="size-3.5" />}
                  </button>
                  <span className={cn("text-sm", todo.done && "text-muted-foreground line-through")}>{todo.title}</span>
                </Form>
              </li>
            ))}
          </ul>
        </CardContent>
      </Card>
    </div>
  );
}
