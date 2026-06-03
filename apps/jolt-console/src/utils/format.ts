export function value(input: number | undefined | null) {
  return input === undefined || input === null ? "--" : String(input);
}

export function formatDuration(totalSeconds: number | undefined | null) {
  const seconds = Number(totalSeconds || 0);
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.floor((seconds % 3600) / 60);
  if (hours > 0) return `${hours}h ${minutes}m`;
  return `${minutes}m ${seconds % 60}s`;
}

export function formatBytes(bytes: number | undefined | null) {
  const numericValue = Number(bytes || 0);
  if (numericValue < 1024) return `${numericValue} B`;

  const units = ["KB", "MB", "GB", "TB"];
  let size = numericValue / 1024;
  let unit = 0;

  while (size >= 1024 && unit < units.length - 1) {
    size /= 1024;
    unit += 1;
  }

  return `${size.toFixed(size >= 10 ? 1 : 2)} ${units[unit]}`;
}

export function shortId(input: string, head = 12, tail = 6) {
  if (input.length <= head + tail + 3) return input;
  return `${input.slice(0, head)}...${input.slice(-tail)}`;
}
