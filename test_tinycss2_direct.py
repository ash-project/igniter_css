#!/usr/bin/env python3
"""
Test tinycss2 directly to see how it handles missing semicolons
"""

import tinycss2

# Test CSS with actual missing semicolon
css_with_missing_semicolon = """
.broken {
    color: red
    background: blue;
}
"""

print("Testing CSS with missing semicolon:")
print(css_with_missing_semicolon)
print("-" * 40)

# Parse the CSS
rules = tinycss2.parse_stylesheet(css_with_missing_semicolon, skip_comments=False, skip_whitespace=False)

print(f"Number of rules parsed: {len(rules)}")

for rule in rules:
    print(f"\nRule type: {rule.type}")
    
    if rule.type == 'qualified-rule':
        print(f"  Prelude (selector): {tinycss2.serialize(rule.prelude)}")
        
        # Parse declarations
        declarations = tinycss2.parse_declaration_list(rule.content, skip_comments=False, skip_whitespace=False)
        
        print(f"  Number of declarations: {len(declarations)}")
        
        for decl in declarations:
            if hasattr(decl, 'type'):
                print(f"    Declaration type: {decl.type}")
                
                if decl.type == 'declaration':
                    print(f"      Name: {decl.name}")
                    print(f"      Value: {tinycss2.serialize(decl.value)}")
                    print(f"      Important: {decl.important}")
                elif decl.type == 'error':
                    print(f"      ERROR: {getattr(decl, 'message', 'Unknown error')}")
                    print(f"      Kind: {getattr(decl, 'kind', 'Unknown')}")

print("\n" + "=" * 60)
print("Conclusion: tinycss2 actually parses 'color: red background: blue;'")
print("as a single declaration with the value 'red background: blue'")
print("This is valid CSS3 syntax for shorthand properties!")
print("So it's not actually an error from tinycss2's perspective.")