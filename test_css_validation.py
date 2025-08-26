#!/usr/bin/env python3
"""
Direct Python test to verify CSS comment handling fix
"""

import sys
import os

# Add the css_tools to the path
sys.path.insert(0, '/Users/shahryar/Desktop/igniter_css/plibs/css_tools/src')

from css_tools.extractor import validate_css

def test_css_validation():
    """Test various CSS with comments to ensure validation works"""
    
    test_cases = [
        {
            "name": "CSS with inline comments after semicolons",
            "css": """
.hide-scrollbar {
    -ms-overflow-style: none; /* Internet Explorer 10+ */
    scrollbar-width: none; /* Firefox */
}
""",
            "should_pass": True
        },
        {
            "name": "CSS with webkit scrollbar and comments",
            "css": """
.hide-scrollbar::-webkit-scrollbar {
    display: none; /* Safari and Chrome */
}
""",
            "should_pass": True
        },
        {
            "name": "CSS with multiple inline comments",
            "css": """
.element {
    color: red; /* Basic comment */
    background: #fff; /* Hex color comment */
    padding: 10px; /* Number with unit */
    margin: 0; /* Zero value */
    border: 1px solid #ccc; /* Multiple values */
    font-family: "Arial", sans-serif; /* String value */
}
""",
            "should_pass": True
        },
        {
            "name": "CSS with multiline comments",
            "css": """
/* 
 * This is a multi-line comment
 * that spans several lines
 */
.container {
    width: 100%; /* Full width */
    max-width: 1200px; /* Maximum width for larger screens */
}
""",
            "should_pass": True
        },
        {
            "name": "CSS with special characters in comments",
            "css": """
.element {
    content: "→"; /* Arrow symbol: → */
    font-size: 16px; /* Size in px (pixels) */
    width: calc(100% - 20px); /* 100% minus padding */
    color: #ff0000; /* RGB: 255, 0, 0 */
}
""",
            "should_pass": True
        },
        {
            "name": "CSS with unbalanced braces (should fail)",
            "css": """
.broken {
    color: red;
    background: blue;

""",
            "should_pass": False
        }
    ]
    
    print("=" * 60)
    print("Testing CSS Validation with Comments")
    print("=" * 60)
    
    passed = 0
    failed = 0
    
    for i, test_case in enumerate(test_cases, 1):
        print(f"\nTest {i}: {test_case['name']}")
        print("-" * 40)
        
        try:
            result = validate_css(test_case['css'])
            
            if test_case['should_pass']:
                print("✅ PASSED: CSS validated successfully")
                passed += 1
            else:
                print("❌ FAILED: Expected validation to fail but it passed")
                failed += 1
                
        except Exception as e:
            if not test_case['should_pass']:
                print(f"✅ PASSED: Correctly failed with: {str(e)}")
                passed += 1
            else:
                print(f"❌ FAILED: {str(e)}")
                failed += 1
    
    print("\n" + "=" * 60)
    print(f"Results: {passed} passed, {failed} failed out of {len(test_cases)} tests")
    
    if failed == 0:
        print("🎉 All tests PASSED! The CSS comment fix is working correctly!")
    else:
        print("⚠️ Some tests failed. Check the output above.")
        sys.exit(1)

if __name__ == "__main__":
    test_css_validation()