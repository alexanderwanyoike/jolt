import { Notebook } from "./data-sdk";

export async function createFirstNote() {
  const notebook = Notebook.test({ identity: "alice.jolt" });
  const item = await notebook.notes.create({
    text: "Hello, Jolt!",
    createdAt: new Date(),
  });

  console.log(item.value.text);
  return item;
}
