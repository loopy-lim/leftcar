import { mount } from "nsh";
import { NavPage, NavStack, createNavStack } from "nsh/base";
import { CatalogScreen } from "./catalog/CatalogScreen";
import { t } from "./i18n";

type Route = "catalog";

function App() {
  const stack = createNavStack<Route>(["catalog"]);
  return (
    <NavStack state={stack} class="flex-1">
      {() => (
        <NavPage title={t().viewer.navCatalog} headerClass="bg-canvas text-ink tint-ink">
          <CatalogScreen />
        </NavPage>
      )}
    </NavStack>
  );
}

mount(App);
