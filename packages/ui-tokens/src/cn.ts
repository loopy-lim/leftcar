import { clsx, type ClassValue } from "clsx";
import { extendTailwindMerge } from "tailwind-merge";

const merge = extendTailwindMerge({
  extend: { theme: { text: ["caption", "label", "body", "title", "subtitle", "heading", "code"] } },
});

export function cn(...inputs: ClassValue[]): string {
  return merge(clsx(inputs));
}
