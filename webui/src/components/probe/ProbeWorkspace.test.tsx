import { renderToStaticMarkup } from "react-dom/server";
import { expect, test } from "vitest";
import { DshHostCard } from "./AgentHookHostCard";

test("DSH displays the configured path without unused controls", () => {
  const markup = renderToStaticMarkup(<DshHostCard sessionsPath="/custom/sandbox/.dsh/sessions" />);
  expect(markup).toContain("/custom/sandbox/.dsh/sessions");
  expect(markup).toContain("原生直连");
  expect(markup).not.toContain("<button");
  expect(markup).not.toContain("disabled");
});
