// This is the opposite of basic/foreign_jsx_transform test case.
import React from 'react';

const App = () => {
    return (
        <div>
            Verbatim module syntax Test Case
        </div>
    )
}

export enum RefType {
    property = '11' as any,
    event = '22' as any,
}

console.log(RefType.property, RefType.event);

export default App;