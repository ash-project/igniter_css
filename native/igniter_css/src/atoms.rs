// SPDX-FileCopyrightText: 2025 igniter_css contributors <https://github.com/ash-project/igniter_css/graphs/contributors>
//
// SPDX-License-Identifier: MIT

rustler::atoms! {
    // Status atoms
    ok,
    error,

    // One atom per NIF, so `IgniterCss.Helpers.normalize_output/2` can label
    // the result with the operation that produced it.
    ensure_at_rule_nif,
    ensure_at_rule_block_nif,
    remove_at_rule_nif,
    has_at_rule_nif,
    add_import_nif,
    remove_import_nif,

    ensure_rule_nif,
    remove_rule_nif,
    replace_rule_body_nif,
    append_raw_to_rule_nif,
    has_rule_nif,
    list_selectors_nif,

    set_declaration_nif,
    remove_declaration_nif,
    get_declaration_nif,
    has_declaration_nif,
    get_rule_declarations_nif,
    add_vendor_prefixes_nif,

    sort_properties_nif,
    remove_duplicates_nif,

    analyze_nif,
    get_at_rules_nif,
    validate_nif,
    extract_colors_nif,
    extract_media_queries_nif,
    extract_animations_nif,

    minify_nif,
    beautify_nif,
    merge_stylesheets_nif,
}
