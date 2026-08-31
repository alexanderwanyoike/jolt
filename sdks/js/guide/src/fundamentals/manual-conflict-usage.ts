import { Notebook } from "./manual-conflicts";

export async function resolveConcurrentEdits(
  resolution: "choose-phone" | "combine",
) {
  const world = Notebook.testWorld();
  const phone = world.device("alice.jolt", "phone");
  const laptop = world.device("alice.jolt", "laptop");
  const created = await phone.notes.create({ text: "Original", pinned: false });

  await world.sync();
  const phoneCopy = await phone.notes.get(created.ref);
  const laptopCopy = await laptop.notes.get(created.ref);
  if (!phoneCopy.isPresent() || !laptopCopy.isPresent()) {
    throw new Error("Expected both devices to have the note");
  }

  await phoneCopy.update({ text: "Phone edit" });
  await laptopCopy.update({ text: "Laptop edit" });
  await world.sync();

  const conflict = await phone.notes.get(created.ref);
  if (!conflict.isConflicted()) {
    throw new Error("Expected a Manual conflict");
  }

  if (resolution === "combine") {
    return conflict.resolve({ text: "Combined edit", pinned: false });
  }

  const phoneEdit = conflict.alternatives.find(alternative => (
    alternative.isPresent() && alternative.value.text === "Phone edit"
  ));
  if (phoneEdit === undefined || !phoneEdit.isPresent()) {
    throw new Error("Expected the phone edit");
  }
  return conflict.choose(phoneEdit);
}
