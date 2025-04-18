"""CSS extraction utilities using tinycss2."""

import tinycss2
import re
from typing import Dict, List, Any, Tuple, Optional, Union, Set
from .parser import parse_stylesheet, get_selector_text, get_rule_declarations


def extract_colors(css: Union[str, bytes]) -> Dict[str, List[str]]:
    """
    Extract all color values from CSS, including those in nested selectors.

    Args:
        css: The CSS code as string or bytes

    Returns:
        Dictionary mapping selectors to their color properties

    Raises:
        Exception: If the CSS cannot be properly parsed
    """
    if isinstance(css, bytes):
        css = css.decode('utf-8')

    # Validate CSS syntax before proceeding
    if css.count('{') != css.count('}'):
        raise Exception("CSS syntax error: Unbalanced braces")

    # Parse CSS for analysis
    rules = parse_stylesheet(css)

    # Check for parse errors
    for rule in rules:
        if hasattr(rule, 'type') and rule.type == 'error':
            raise Exception(f"CSS parse error: {getattr(rule, 'message', 'Unknown error')}")

    colors = {}

    # Regular expressions for different color formats
    hex_pattern = r'#([0-9a-fA-F]{3}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8})'
    rgb_pattern = r'rgb\(\s*\d+\s*,\s*\d+\s*,\s*\d+\s*\)'
    rgba_pattern = r'rgba\(\s*\d+\s*,\s*\d+\s*,\s*\d+\s*,\s*[0-9.]+\s*\)'
    hsl_pattern = r'hsl\(\s*\d+\s*,\s*\d+%\s*,\s*\d+%\s*\)'
    hsla_pattern = r'hsla\(\s*\d+\s*,\s*\d+%\s*,\s*\d+%\s*,\s*[0-9.]+\s*\)'

    color_properties = [
        'color', 'background-color', 'border-color', 'border-top-color',
        'border-right-color', 'border-bottom-color', 'border-left-color',
        'outline-color', 'text-decoration-color', 'box-shadow', 'text-shadow'
    ]

    # Recursive function to process rules
    def process_rules(rule_list):
        for rule in rule_list:
            if rule.type == "qualified-rule":
                selector = get_selector_text(rule)
                declarations = get_rule_declarations(rule)
                for decl in declarations:
                    if decl.type == "declaration":
                        value = tinycss2.serialize(decl.value).strip()
                        # Check if it's a color property or has a color value
                        is_color_property = decl.name in color_properties
                        has_color_value = (
                            re.search(hex_pattern, value) or
                            re.search(rgb_pattern, value) or
                            re.search(rgba_pattern, value) or
                            re.search(hsl_pattern, value) or
                            re.search(hsla_pattern, value) or
                            value in ['black', 'white', 'red', 'green', 'blue', 'yellow',
                                     'purple', 'orange', 'brown', 'gray', 'transparent']
                        )
                        if is_color_property or has_color_value:
                            if selector not in colors:
                                colors[selector] = []
                            colors[selector].append(f"{decl.name}: {value}")

            # Process media queries and other at-rules with nested content
            elif rule.type == "at-rule" and rule.content is not None:
                # Parse nested rules
                nested_rules = parse_stylesheet(tinycss2.serialize(rule.content))
                # Recursively process nested rules
                process_rules(nested_rules)

    # Start processing rules
    process_rules(rules)

    return colors

def extract_media_queries(css: Union[str, bytes]) -> Dict[str, List[Dict[str, Any]]]:
    """
    Extract all media queries and their contents.

    Args:
        css: The CSS code as string or bytes

    Returns:
        Dictionary mapping media query conditions to their rules

    Raises:
        Exception: If the CSS cannot be properly parsed
    """
    if isinstance(css, bytes):
        css = css.decode('utf-8')

    # Validate CSS syntax before proceeding
    # Check for unbalanced braces - a common CSS error
    if css.count('{') != css.count('}'):
        raise Exception("CSS syntax error: Unbalanced braces")

    rules = parse_stylesheet(css)

    # Check for parse errors
    for rule in rules:
        if hasattr(rule, 'type') and rule.type == 'error':
            raise Exception(f"CSS parse error: {getattr(rule, 'message', 'Unknown error')}")

    media_queries = {}

    for rule in rules:
        if rule.type == "at-rule" and rule.lower_at_keyword == "media":
            condition = tinycss2.serialize(rule.prelude).strip()

            if condition not in media_queries:
                media_queries[condition] = []

            # Parse the content of the media query
            if hasattr(rule, 'content') and rule.content:
                try:
                    inner_rules = tinycss2.parse_stylesheet(
                        rule.content, skip_whitespace=False, skip_comments=False
                    )

                    # Check for parse errors in inner rules
                    for inner_rule in inner_rules:
                        if hasattr(inner_rule, 'type') and inner_rule.type == 'error':
                            raise Exception(f"CSS parse error in media query: {getattr(inner_rule, 'message', 'Unknown error')}")

                    for inner_rule in inner_rules:
                        if inner_rule.type == "qualified-rule":
                            selector = get_selector_text(inner_rule)
                            declarations = get_rule_declarations(inner_rule)

                            props = {}
                            for decl in declarations:
                                if decl.type == "declaration":
                                    props[decl.name] = tinycss2.serialize(decl.value).strip()

                            media_queries[condition].append({
                                "selector": selector,
                                "properties": props
                            })
                except Exception as e:
                    raise Exception(f"Error parsing media query content: {str(e)}")

    return media_queries


def validate_css(css: Union[str, bytes]) -> str:
    """
    Validates CSS syntax and returns decoded string.

    Args:
        css: The CSS code as string or bytes

    Returns:
        Decoded CSS string

    Raises:
        Exception: If the CSS cannot be properly parsed
    """

    if isinstance(css, bytes):
        css = css.decode('utf-8')

    # Check for unbalanced braces - a common CSS error
    if css.count('{') != css.count('}'):
        raise Exception("CSS syntax error: Unbalanced braces")

    # Parse the CSS to detect syntax errors
    rules = tinycss2.parse_stylesheet(css)
    check_parse_errors(rules)

    # Check each rule for proper declaration syntax
    for rule in rules:
        if rule.type == 'qualified-rule' and rule.content:
            declarations = tinycss2.parse_declaration_list(rule.content)
            check_parse_errors(declarations, "in declaration")

            # Extra check for missing semicolons
            serialized_rule = tinycss2.serialize(rule.content).strip()
            # Look for patterns that suggest missing semicolons
            lines = serialized_rule.split('\n')
            for i, line in enumerate(lines):
                line = line.strip()
                if ':' in line and not line.endswith(';') and not line.endswith('{') and '}' not in line:
                    # Check if this is not the last line or if there's another declaration after it
                    if (i+1 < len(lines) and (':' in lines[i+1] or not lines[i+1].strip().startswith('}'))):
                        raise Exception(f"CSS syntax error: Missing semicolon after '{line}'")

    # Manual check for missing semicolons (line by line)
    lines = css.split('\n')
    for i, line in enumerate(lines):
        line_stripped = line.strip()
        # If it looks like a declaration (has a colon) but doesn't end with a semicolon
        if ':' in line_stripped and not line_stripped.endswith(';') and not line_stripped.endswith('{') and '}' not in line_stripped:
            # Check if next line exists and has content (not a closing brace or empty)
            if i+1 < len(lines) and lines[i+1].strip() and not lines[i+1].strip().startswith('}'):
                raise Exception(f"CSS syntax error: Missing semicolon at line {i+1} after '{line_stripped}'")

    return css

def check_parse_errors(rules, context=""):
    """
    Check for parse errors in a list of CSS rules.

    Args:
        rules: List of CSS rules
        context: Optional context description for error messages

    Raises:
        Exception: If any parse errors are found
    """
    for rule in rules:
        if hasattr(rule, 'type') and rule.type == 'error':
            prefix = f"CSS parse error {context}: " if context else "CSS parse error: "
            raise Exception(f"{prefix}{getattr(rule, 'message', 'Unknown error')}")


def extract_keyframes(rule):
    """
    Extract keyframes from a @keyframes rule.

    Args:
        rule: The @keyframes at-rule

    Returns:
        Dictionary mapping percentages to property dictionaries
    """
    keyframes = {}

    if not (hasattr(rule, 'content') and rule.content):
        return keyframes

    try:
        keyframe_rules = tinycss2.parse_stylesheet(
            rule.content, skip_whitespace=False, skip_comments=False
        )

        # Check for parse errors in keyframe rules
        check_parse_errors(keyframe_rules, "in @keyframes")

        for keyframe_rule in keyframe_rules:
            if keyframe_rule.type == "qualified-rule":
                # The "selector" for keyframes is the percentage or keywords (from/to)
                percentage = get_selector_text(keyframe_rule)
                declarations = get_rule_declarations(keyframe_rule)

                props = {}
                for decl in declarations:
                    if decl.type == "declaration":
                        props[decl.name] = tinycss2.serialize(decl.value).strip()

                keyframes[percentage] = props
    except Exception as e:
        raise Exception(f"Error parsing @keyframes content: {str(e)}")

    return keyframes


def find_animation_usage(rules):
    """
    Find all elements using animations.

    Args:
        rules: List of CSS rules

    Returns:
        Dictionary mapping animation names to lists of selectors using them
    """
    animation_usage = {}

    # List of animation-related properties (including vendor prefixes)
    animation_properties = [
        "animation", "animation-name",
        "-webkit-animation", "-webkit-animation-name",
        "-moz-animation", "-moz-animation-name",
        "-ms-animation", "-ms-animation-name",
        "-o-animation", "-o-animation-name"
    ]

    for rule in rules:
        if rule.type == "qualified-rule":
            selector = get_selector_text(rule)
            declarations = get_rule_declarations(rule)

            for decl in declarations:
                if decl.type == "declaration" and decl.name in animation_properties:
                    value = tinycss2.serialize(decl.value).strip()
                    # Simple extraction, might need more complex parsing for multiple animations
                    animation_name = value.split()[0]

                    # Normalize animation name (remove quotes if present)
                    animation_name = animation_name.strip("'\"")

                    if animation_name not in animation_usage:
                        animation_usage[animation_name] = []
                    animation_usage[animation_name].append(selector)

    return animation_usage


def extract_animations(css: Union[str, bytes]) -> Dict[str, Dict[str, Any]]:
    """
    Extract all CSS animations and keyframes, including vendor-prefixed ones.

    Args:
        css: The CSS code as string or bytes

    Returns:
        Dictionary mapping animation names to their keyframes

    Raises:
        Exception: If the CSS cannot be properly parsed
    """
    # Validate and decode CSS
    if isinstance(css, bytes):
        css = css.decode('utf-8')

    # Validate CSS syntax
    if css.count('{') != css.count('}'):
        raise Exception("CSS syntax error: Unbalanced braces")

    # Parse CSS
    rules = parse_stylesheet(css)

    # Check for parse errors
    for rule in rules:
        if hasattr(rule, 'type') and rule.type == 'error':
            raise Exception(f"CSS parse error: {getattr(rule, 'message', 'Unknown error')}")

    animations = {}

    # List of possible keyframes at-keywords (standard and vendor prefixed)
    keyframes_keywords = [
        "keyframes",
        "-webkit-keyframes",
        "-moz-keyframes",
        "-ms-keyframes",
        "-o-keyframes"
    ]

    # First pass: Find all @keyframes rules (including vendor prefixed)
    for rule in rules:
        if rule.type == "at-rule":
            # Check if this is a keyframes rule (standard or vendor prefixed)
            is_keyframes = False
            animation_name = ""

            # Check against all possible keyframes at-keywords
            for keyword in keyframes_keywords:
                if rule.lower_at_keyword == keyword or rule.at_keyword.lower() == keyword:
                    is_keyframes = True
                    break

            if is_keyframes:
                # Extract animation name
                animation_name = tinycss2.serialize(rule.prelude).strip()
                # Normalize animation name (remove quotes if present)
                animation_name = animation_name.strip("'\"")

                # Extract keyframes
                keyframes = {}
                if hasattr(rule, 'content') and rule.content:
                    try:
                        keyframe_rules = tinycss2.parse_stylesheet(
                            rule.content, skip_whitespace=False, skip_comments=False
                        )

                        # Check for parse errors in keyframe rules
                        for keyframe_rule in keyframe_rules:
                            if hasattr(keyframe_rule, 'type') and keyframe_rule.type == 'error':
                                raise Exception(f"CSS parse error in @keyframes: {getattr(keyframe_rule, 'message', 'Unknown error')}")

                        for keyframe_rule in keyframe_rules:
                            if keyframe_rule.type == "qualified-rule":
                                # The "selector" for keyframes is the percentage or keywords (from/to)
                                percentage = get_selector_text(keyframe_rule)
                                declarations = get_rule_declarations(keyframe_rule)

                                props = {}
                                for decl in declarations:
                                    if decl.type == "declaration":
                                        props[decl.name] = tinycss2.serialize(decl.value).strip()

                                keyframes[percentage] = props
                    except Exception as e:
                        raise Exception(f"Error parsing @keyframes content: {str(e)}")

                animations[animation_name] = keyframes

    # Second pass: Find all elements using animations
    animation_usage = find_animation_usage(rules)

    # Combine the results
    result = {}
    for name, keyframes in animations.items():
        result[name] = {
            "keyframes": keyframes,
            "used_by": animation_usage.get(name, [])
        }

    return result

def extract_unused_selectors(css: Union[str, bytes], html_content: str) -> List[str]:
    """
    Extract CSS selectors that are not used in the given HTML content.

    Args:
        css: The CSS code as string or bytes
        html_content: The HTML content to check against

    Returns:
        List of unused selectors
    """
    if isinstance(css, bytes):
        css = css.decode('utf-8')

    rules = parse_stylesheet(css)
    all_selectors = []
    unused_selectors = []

    for rule in rules:
        if rule.type == "qualified-rule":
            selector = get_selector_text(rule)
            # Skip pseudo-elements and pseudo-classes for simplicity
            base_selector = re.sub(r'::?[a-zA-Z-]+(\([^)]*\))?', '', selector)

            # Process complex selectors
            parts = re.split(r'\s*[,>+~]\s*', base_selector)
            for part in parts:
                part = part.strip()
                if part and part not in all_selectors:
                    all_selectors.append(part)

    # Basic check for unused selectors
    for selector in all_selectors:
        # Extract class and ID selectors
        if selector.startswith('.'):
            # Class selector
            class_name = selector[1:]
            if f'class="{class_name}"' not in html_content and f"class='{class_name}'" not in html_content:
                unused_selectors.append(selector)
        elif selector.startswith('#'):
            # ID selector
            id_name = selector[1:]
            if f'id="{id_name}"' not in html_content and f"id='{id_name}'" not in html_content:
                unused_selectors.append(selector)
        else:
            # Element selector - more complex, would need proper HTML parsing
            pass

    return unused_selectors


def extract_fonts(css: Union[str, bytes]) -> Dict[str, List[Dict[str, Any]]]:
    """
    Extract all font-related properties, including those in nested rules and media queries.

    Args:
        css: The CSS code as string or bytes

    Returns:
        Dictionary mapping selectors to their font properties

    Raises:
        Exception: If the CSS cannot be properly parsed or has invalid syntax
    """
    if isinstance(css, bytes):
        css = css.decode('utf-8')

    # Parse CSS for analysis
    rules = parse_stylesheet(css)

    # Check for parse errors
    for rule in rules:
        if hasattr(rule, 'type') and rule.type == 'error':
            raise Exception(f"CSS parse error: {getattr(rule, 'message', 'Unknown error')}")

    # Validate declarations
    for rule in rules:
        if rule.type == "qualified-rule":
            declarations = get_rule_declarations(rule)
            for decl in declarations:
                if decl.type == "error":
                    raise Exception(f"CSS parse error in declaration: {getattr(decl, 'message', 'Unknown error')}")

    fonts = {}

    # List of font-related properties
    font_properties = [
        'font', 'font-family', 'font-size', 'font-weight', 'font-style',
        'font-variant', 'line-height', 'text-transform', 'letter-spacing'
    ]

    # Recursive function to process rules
    def process_rules(rule_list, parent_selector=""):
        for rule in rule_list:
            if rule.type == "qualified-rule":
                selector = get_selector_text(rule)
                # Handle nested selectors by combining with parent selector
                full_selector = f"{parent_selector} {selector}".strip() if parent_selector else selector

                declarations = get_rule_declarations(rule)
                font_decls = []

                for decl in declarations:
                    if decl.type == "declaration" and decl.name in font_properties:
                        value = tinycss2.serialize(decl.value).strip()
                        font_decls.append({
                            "property": decl.name,
                            "value": value
                        })

                if font_decls:
                    if full_selector not in fonts:
                        fonts[full_selector] = []
                    fonts[full_selector].extend(font_decls)

                # Process nested rules within this rule
                if hasattr(rule, 'content') and rule.content:
                    nested_rules = parse_stylesheet(tinycss2.serialize(rule.content))
                    process_rules(nested_rules, full_selector)

            # Process media queries and other at-rules with nested content
            elif rule.type == "at-rule" and rule.content is not None:
                # For media queries, we want to keep the selector as is
                if rule.lower_at_keyword == "media":
                    # Parse nested rules
                    nested_rules = parse_stylesheet(tinycss2.serialize(rule.content))
                    # Process nested rules with the same parent selector
                    process_rules(nested_rules, parent_selector)
                else:
                    # For other at-rules, we want to combine the selectors
                    nested_rules = parse_stylesheet(tinycss2.serialize(rule.content))
                    process_rules(nested_rules, parent_selector)

    # Start processing rules
    process_rules(rules)

    return fonts

def extract_selectors_by_property(css: Union[str, bytes], property_name: str) -> Dict[str, str]:
    """
    Extract all selectors that use a specific CSS property and their values.

    Args:
        css: The CSS code as string or bytes
        property_name: The name of the property to extract (case-insensitive)

    Returns:
        Dictionary mapping selectors to their property values

    Raises:
        Exception: If the CSS cannot be properly parsed
    """
    if isinstance(css, bytes):
        css = css.decode('utf-8')

    # Validate CSS syntax before proceeding
    if css.count('{') != css.count('}'):
        raise Exception("CSS syntax error: Unbalanced braces")

    # Parse CSS for analysis
    rules = parse_stylesheet(css)

    # Check for parse errors
    for rule in rules:
        if hasattr(rule, 'type') and rule.type == 'error':
            raise Exception(f"CSS parse error: {getattr(rule, 'message', 'Unknown error')}")

    selectors = {}

    # Recursive function to process rules
    def process_rules(rule_list):
        for rule in rule_list:
            if rule.type == "qualified-rule":
                selector = get_selector_text(rule)
                declarations = get_rule_declarations(rule)

                for decl in declarations:
                    if decl.type == "declaration" and decl.name.lower() == property_name.lower():
                        value = tinycss2.serialize(decl.value).strip()
                        if decl.important:
                            value += " !important"
                        selectors[selector] = value

            # Process media queries and other at-rules with nested content
            elif rule.type == "at-rule" and rule.content is not None:
                # Parse nested rules
                nested_rules = parse_stylesheet(tinycss2.serialize(rule.content))
                # Recursively process nested rules
                process_rules(nested_rules)

    # Start processing rules
    process_rules(rules)

    return selectors
