"use client";
import { createTokenizer } from "@orama/tokenizers/mandarin";
import { useDocsSearch } from "fumadocs-core/search/client";
import { create } from "zbsearch";
import {
  SearchDialog,
  SearchDialogClose,
  SearchDialogContent,
  SearchDialogHeader,
  SearchDialogIcon,
  SearchDialogInput,
  SearchDialogList,
  SearchDialogOverlay,
  type SharedProps,
} from "fumadocs-ui/components/dialog/search";
import { useI18n } from "fumadocs-ui/contexts/i18n";

export default function DefaultSearchDialog(props: SharedProps) {
  const { locale } = useI18n(); // (optional) for i18n
  const { search, setSearch, query } = useDocsSearch({
    type: "static",
    locale,
    // The zh index is serialized with a mandarin tokenizer on the server
    // (see src/routes/api/search.ts); the default tokenizer leaves CJK
    // queries unsegmented, so they match none of the segmented index tokens.
    initDB: (lang) =>
      create({
        schema: { _: "string" },
        ...(lang === "zh" ? { components: { tokenizer: createTokenizer() } } : {}),
      }),
  });

  return (
    <SearchDialog
      search={search}
      onSearchChange={setSearch}
      isLoading={query.isLoading}
      {...props}
    >
      <SearchDialogOverlay />
      <SearchDialogContent>
        <SearchDialogHeader>
          <SearchDialogIcon />
          <SearchDialogInput />
          <SearchDialogClose />
        </SearchDialogHeader>
        <SearchDialogList items={query.data !== "empty" ? query.data : null} />
      </SearchDialogContent>
    </SearchDialog>
  );
}
