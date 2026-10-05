/** Built-in sequences, served from `public/examples/` and fetched on demand. */
export interface Example {
  file: string;
  label: string;
}

export const EXAMPLES: Example[] = [
  { file: "flash_je.seq", label: "FLASH (96 lines)" },
  { file: "grappa_acs.seq", label: "GRAPPA with ACS lines" },
  { file: "tse.seq", label: "TSE (dummy + 4 shots × 16 echoes)" },
  { file: "seq_make_radial.seq", label: "Radial (4 spokes)" },
  { file: "flash_repeated.seq", label: "FLASH ×200, 96k blocks (2 MB)" },
];

export async function fetchExample(example: Example): Promise<string> {
  const response = await fetch(`./examples/${example.file}`);
  if (!response.ok) throw new Error(`HTTP ${response.status} fetching ${example.file}`);
  return response.text();
}
