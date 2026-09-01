import { Notebook } from "./manual-conflicts";

export async function resolveConcurrentEdits(
  resolution: "choose-workstation" | "combine",
) {
  const world = Notebook.testWorld();
  const workstation = world.device("alice.jolt", "workstation");
  const laptop = world.device("alice.jolt", "laptop");
  const created = await workstation.notes.create({ text: "Original", pinned: false });

  await world.sync();
  const workstationCopy = await workstation.notes.get(created.ref);
  const laptopCopy = await laptop.notes.get(created.ref);
  if (!workstationCopy.isPresent() || !laptopCopy.isPresent()) {
    throw new Error("Expected both devices to have the note");
  }

  await workstationCopy.update({ text: "Workstation edit" });
  await laptopCopy.update({ text: "Laptop edit" });
  await world.sync();

  const conflict = await workstation.notes.get(created.ref);
  if (!conflict.isConflicted()) {
    throw new Error("Expected a Manual conflict");
  }

  if (resolution === "combine") {
    return conflict.resolve({ text: "Combined edit", pinned: false });
  }

  const workstationEdit = conflict.alternatives.find(alternative => (
    alternative.isPresent() && alternative.value.text === "Workstation edit"
  ));
  if (workstationEdit === undefined || !workstationEdit.isPresent()) {
    throw new Error("Expected the workstation edit");
  }
  return conflict.choose(workstationEdit);
}
