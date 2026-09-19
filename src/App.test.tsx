import { render, screen } from "@testing-library/react";
import { expect, test } from "vitest";
import { App } from "./App";

test("the window carries the product name", () => {
  render(<App />);
  expect(screen.getByRole("heading", { name: "Muzn Download Manager" })).toBeInTheDocument();
});
