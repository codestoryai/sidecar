use async_trait::async_trait;
use serde_xml_rs::from_str;
use std::{
    collections::{HashMap, HashSet},
    sync::Arc,
};

use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage},
};

use crate::agentic::{
    symbol::identifier::{LLMProperties, Snippet},
    tool::{
        filtering::{
            broker::{
                CodeToEditFilterFormatter, CodeToEditFilterRequest, CodeToEditFilterResponse,
                CodeToEditList, CodeToEditSymbolRequest, CodeToEditSymbolResponse,
                CodeToNotEditList, CodeToProbeFilterResponse, CodeToProbeList,
                CodeToProbeSubSymbolList, CodeToProbeSubSymbolRequest, CodeToProbeSymbolResponse,
                SnippetWithReason,
            },
            errors::CodeToEditFilteringError,
        },
        jitter::jitter_sleep,
    },
};

pub struct AnthropicCodeToEditFormatter {
    llm_broker: Arc<LLMBroker>,
    fail_over_llm: LLMProperties,
}

impl AnthropicCodeToEditFormatter {
    pub fn new(llm_broker: Arc<LLMBroker>, fail_over_llm: LLMProperties) -> Self {
        Self {
            llm_broker,
            fail_over_llm,
        }
    }

    fn example_message_for_filtering_code_edit_block(&self) -> String {
        r#"<example>
<user_query>
We want to add a new method to add a new shipment made by the company.
</user_query>

<rerank_list>
<rerank_entry>
<id>
0
</id>
<content>
Code Location: company.rs
```rust
struct Company {
    name: String,
    shipments: usize,
    size: usize,
}
```
</content>
</rerank_entry>
<rerank_entry>
<id>
1
</id>
<content>
Code Location: company_metadata.rs
```rust
impl Compnay {
    fn name(&self) -> &str {
        &self.name
    }

    fn size(&self) -> usize {
        self.size
    }
}
</content>
</rerank_entry>
<rerank_entry>
<id>
2
</id>
<content>
Code Location: company_shipments.rs
```rust
impl Company {
    fn get_snipments(&self) -> usize {
        self.shipments
    }
}
```
</content>
</rerank_entry>
</rerank_list>

Your reply should be:

<thinking>
The company_shipment implementation block handles everything related to the shipments of the company, so we want to edit that.
</thinking>

<code_to_edit_list>
<code_to_edit>
<id>
2
</id>
<reason_to_edit>
The company_shipment.rs implementation block of Company contains all the relevant code for the shipment tracking of the Company, so that's what we want to edit.
</reason_to_edit>
<id>
</code_to_edit>
</code_to_edit_list>
</example>"#
            .to_owned()
    }

    fn _example_message(&self) -> String {
        r#"<example>
<user_query>
The checkout process is broken. After entering payment info, the order doesn't get created and the user sees an error page.
</user_query>
<rerank_list>
<rerank_entry>
<id>
0
</id>
<content>
Code Location: auth.js:5-30
```typescript
const bcrypt = require('bcryptjs');
const User = require('../models/user');
router.post('/register', async (req, res) => {{
const {{ email, password, name }} = req.body;
try {{
let user = await User.findOne({{ email }});
if (user) {{
return res.status(400).json({{ message: 'User already exists' }});
    }}
user = new User({{
        email,
        password,
        name
    }});
    const salt = await bcrypt.genSalt(10);
    user.password = await bcrypt.hash(password, salt);
    await user.save();
    req.session.user = user;
res.json({{ message: 'Registration successful', user }});
}} catch (err) {{
    console.error(err);
res.status(500).json({{ message: 'Server error' }});
    }}  
}});

router.post('/login', async (req, res) => {{
const {{ email, password }} = req.body;

try {{
const user = await User.findOne({{ email }});
if (!user) {{
return res.status(400).json({{ message: 'Invalid credentials' }});
    }}

    const isMatch = await bcrypt.compare(password, user.password);
if (!isMatch) {{
return res.status(400).json({{ message: 'Invalid credentials' }});  
    }}

    req.session.user = user;
res.json({{ message: 'Login successful', user }});
}} catch (err) {{
    console.error(err);
res.status(500).json({{ message: 'Server error' }});
    }}
}});
```
</content>
</rerank_entry>
<rerank_entry>
<id>
1
</id>
<content>
Code Location: cart_model.js:1-20
```typescript
const mongoose = require('mongoose');
const cartSchema = new mongoose.Schema({{
user: {{
    type: mongoose.Schema.Types.ObjectId,
    ref: 'User',
    required: true
    }},
items: [{{
product: {{
        type: mongoose.Schema.Types.ObjectId,
        ref: 'Product'
    }},
    quantity: Number,
    price: Number  
    }}]
}}, {{ timestamps: true }});
cartSchema.virtual('totalPrice').get(function() {{
    return this.items.reduce((total, item) => total + item.price * item.quantity, 0);
}});
module.exports = mongoose.model('Cart', cartSchema);
```
</content>
</rerank_entry>
<rerank_entry>
<id>
2
</id>
<content>
Code Location: order.js:5-25
```typescript
const Order = require('../models/order');
router.get('/', async (req, res) => {{
try {{
const orders = await Order.find({{ user: req.user._id }}).sort('-createdAt');
    res.json(orders);
}} catch (err) {{
    console.error(err);
res.status(500).json({{ message: 'Server error' }});
    }}
}});
router.get('/:id', async (req, res) => {{
try {{
const order = await Order.findOne({{ _id: req.params.id, user: req.user._id }});
if (!order) {{
return res.status(404).json({{ message: 'Order not found' }});
    }}
    res.json(order);
}} catch (err) {{
    console.error(err);
res.status(500).json({{ message: 'Server error' }});
    }}  
}});
</content>
</rerank_entry>
<rerank_entry>
<id>
3
</id>
<content>
Code Location: checkout.js:5-30
```typescript
router.post('/submit', async (req, res) => {{
const {{ cartId, paymentInfo }} = req.body;
try {{
    const cart = await Cart.findById(cartId).populate('items.product');
if (!cart) {{
return res.status(404).json({{ message: 'Cart not found' }});
    }}
const order = new Order({{
        user: req.user._id,
        items: cart.items,
        total: cart.totalPrice,
        paymentInfo,
    }});
    await order.save();
    await Cart.findByIdAndDelete(cartId);
res.json({{ message: 'Order placed successfully', orderId: order._id }});
}} catch (err) {{
    console.error(err);
res.status(500).json({{ message: 'Server error' }});
    }}
}});
```
</content>
</rerank_entry>
<rerank_entry>
<id>
4
</id>
<content>
Code Location: user_model.js:1-10
const mongoose = require('mongoose');
const userSchema = new mongoose.Schema({{
email: {{
    type: String,
    required: true,
    unique: true
    }},
password: {{
    type: String,
    required: true
    }},
    name: String,
    address: String,
    phone: String,
isAdmin: {{
    type: Boolean,
    default: false  
    }}
}}, {{ timestamps: true }});
module.exports = mongoose.model('User', userSchema);
</content>
</rerank_entry>
<rerank_entry>
<id>
5
</id>
<content>
Code Location: index.js:10-25
```typescript
const express = require('express');
const mongoose = require('mongoose');
const session = require('express-session');
const MongoStore = require('connect-mongo')(session);
const app = express();
mongoose.connect(process.env.MONGO_URI, {{
    useNewUrlParser: true,
    useUnifiedTopology: true
}});
app.use(express.json());
app.use(session({{
    secret: process.env.SESSION_SECRET,
    resave: false,
    saveUninitialized: true,
store: new MongoStore({{ mongooseConnection: mongoose.connection }})
}}));
app.use('/auth', require('./routes/auth'));
app.use('/cart', require('./routes/cart'));  
app.use('/checkout', require('./routes/checkout'));
app.use('/orders', require('./routes/order'));
app.use('/products', require('./routes/product'));
app.use((err, req, res, next) => {{
    console.error(err);
res.status(500).json({{ message: 'Server error' }});
}});
const PORT = process.env.PORT || 5000;
app.listen(PORT, () => console.log(`Server started on port ${{PORT}}`));
```
</content>
</rerank_entry>
<rerank_entry>
<id>
6
</id>
<content>
Code Loction: payment.js:3-20
```typescript
const stripe = require('stripe')(process.env.STRIPE_SECRET_KEY);
router.post('/charge', async (req, res) => {{
const {{ amount, token }} = req.body;
try {{
const charge = await stripe.charges.create({{
        amount,
        currency: 'usd',
        source: token,
        description: 'Example charge'
    }});
res.json({{ message: 'Payment successful', charge }});
}} catch (err) {{
    console.error(err);  
res.status(500).json({{ message: 'Payment failed' }});
    }}
}});
```
</content>
</rerank_entry>
<rerank_entry>
<id>
7
</id>
<content>
Code Loction: product_model.js:1-12
```typescript
const mongoose = require('mongoose');
const productSchema = new mongoose.Schema({{
name: {{
    type: String,
    required: true
    }},
    description: String,
price: {{
    type: Number,
    required: true,
    min: 0
    }},
category: {{
    type: String,
    enum: ['electronics', 'clothing', 'home'],
    required: true  
    }},
stock: {{
    type: Number,
    default: 0,
    min: 0
    }}
}});
module.exports = mongoose.model('Product', productSchema);
```
</content>
</rerank_entry>
<rerank_entry>
<id>
8
</id>
<content>
Code Location: order_model.js:1-15
```typescript
const mongoose = require('mongoose');
const orderSchema = new mongoose.Schema({{
user: {{ 
    type: mongoose.Schema.Types.ObjectId,
    ref: 'User',
    required: true
    }},
items: [{{
product: {{
        type: mongoose.Schema.Types.ObjectId,
        ref: 'Product'
    }},
    quantity: Number,
    price: Number
    }}],
total: {{
    type: Number,
    required: true
    }},
paymentInfo: {{
    type: Object,
    required: true
    }},
status: {{
    type: String,
    enum: ['pending', 'processing', 'shipped', 'delivered'],
    default: 'pending'
    }}
}}, {{ timestamps: true }});
module.exports = mongoose.model('Order', orderSchema);
```
</content>
</rerank_entry>

<rerank_entry>
<id>
9
</id>
<content>
Code Location: cart.js:5-20
```typescript
router.post('/add', async (req, res) => {{
const {{ productId, quantity }} = req.body;
    
try {{
let cart = await Cart.findOne({{ user: req.user._id }});
if (cart) {{
        const itemIndex = cart.items.findIndex(item => item.product == productId);
if (itemIndex > -1) {{
        cart.items[itemIndex].quantity += quantity;
}} else {{
cart.items.push({{ product: productId, quantity, price: product.price }});
        }}
        cart = await cart.save();
}} else {{
cart = await Cart.create({{
        user: req.user._id,
items: [{{ product: productId, quantity, price: product.price }}]
        }});
    }}
    res.json(cart);
}} catch (err) {{
    console.error(err);
res.status(500).json({{ message: 'Server error' }});  
    }}
}});
```
</content>
</rerank_entry>
</rerank_list>

Your reply should be:

<thinking>
We want to get the relevant code for handling the checkout process since that has the error. The checkout and the payment along with how the order schema is handled seems relevant to the user query.
</thinking>

<code_to_edit_list>
<code_to_edit>
<id>
3
</id>
<reason_to_edit>
This code handles the checkout process. It receives the cart ID and payment info from the request body. It finds the cart, creates a new order with the cart items and payment info, saves the order, deletes the cart, and returns the order ID. This is likely where the issue is occurring.
</reason_to_edit>
<id>
</code_to_edit>
<code_to_edit>
<id>
6
</id>
<reason_to_edit>
This code processes the actual payment by creating a Stripe charge. The payment info comes from the checkout process. If the payment fails, that could explain the checkout error, so this is important to investigate.
</reason_to_edit>
</code_to_edit>
<code_to_edit>
<id>
8
</id>
<reason_to_edit>
This defines the schema and model for orders. An order contains references to the user and product items, the total price, payment info, and status. It's important for understanding the structure of an order, but unlikely to contain bugs.
</reason_to_edit>
<code_to_edit>
</code_to_edit_list>
</example>"#.to_owned()
    }

    fn example_message_for_probing(&self) -> String {
        r#"<example>
<user_query>
The checkout process is broken. After entering payment info, the order doesn't get created and the user sees an error page.
</user_query>
<rerank_list>
<rerank_entry>
<id>
0
</id>
<content>
Code Location: auth.js:5-30
```typescript
const bcrypt = require('bcryptjs');
const User = require('../models/user');
router.post('/register', async (req, res) => {{
const {{ email, password, name }} = req.body;
try {{
let user = await User.findOne({{ email }});
if (user) {{
return res.status(400).json({{ message: 'User already exists' }});
    }}
user = new User({{
        email,
        password,
        name
    }});
    const salt = await bcrypt.genSalt(10);
    user.password = await bcrypt.hash(password, salt);
    await user.save();
    req.session.user = user;
res.json({{ message: 'Registration successful', user }});
}} catch (err) {{
    console.error(err);
res.status(500).json({{ message: 'Server error' }});
    }}  
}});

router.post('/login', async (req, res) => {{
const {{ email, password }} = req.body;

try {{
const user = await User.findOne({{ email }});
if (!user) {{
return res.status(400).json({{ message: 'Invalid credentials' }});
    }}

    const isMatch = await bcrypt.compare(password, user.password);
if (!isMatch) {{
return res.status(400).json({{ message: 'Invalid credentials' }});  
    }}

    req.session.user = user;
res.json({{ message: 'Login successful', user }});
}} catch (err) {{
    console.error(err);
res.status(500).json({{ message: 'Server error' }});
    }}
}});
```
</content>
</rerank_entry>
<rerank_entry>
<id>
1
</id>
<content>
Code Location: cart_model.js:1-20
```typescript
const mongoose = require('mongoose');
const cartSchema = new mongoose.Schema({{
user: {{
    type: mongoose.Schema.Types.ObjectId,
    ref: 'User',
    required: true
    }},
items: [{{
product: {{
        type: mongoose.Schema.Types.ObjectId,
        ref: 'Product'
    }},
    quantity: Number,
    price: Number  
    }}]
}}, {{ timestamps: true }});
cartSchema.virtual('totalPrice').get(function() {{
    return this.items.reduce((total, item) => total + item.price * item.quantity, 0);
}});
module.exports = mongoose.model('Cart', cartSchema);
```
</content>
</rerank_entry>
<rerank_entry>
<id>
2
</id>
<content>
Code Location: order.js:5-25
```typescript
const Order = require('../models/order');
router.get('/', async (req, res) => {{
try {{
const orders = await Order.find({{ user: req.user._id }}).sort('-createdAt');
    res.json(orders);
}} catch (err) {{
    console.error(err);
res.status(500).json({{ message: 'Server error' }});
    }}
}});
router.get('/:id', async (req, res) => {{
try {{
const order = await Order.findOne({{ _id: req.params.id, user: req.user._id }});
if (!order) {{
return res.status(404).json({{ message: 'Order not found' }});
    }}
    res.json(order);
}} catch (err) {{
    console.error(err);
res.status(500).json({{ message: 'Server error' }});
    }}  
}});
</content>
</rerank_entry>
<rerank_entry>
<id>
3
</id>
<content>
Code Location: checkout.js:5-30
```typescript
router.post('/submit', async (req, res) => {{
const {{ cartId, paymentInfo }} = req.body;
try {{
    const cart = await Cart.findById(cartId).populate('items.product');
if (!cart) {{
return res.status(404).json({{ message: 'Cart not found' }});
    }}
const order = new Order({{
        user: req.user._id,
        items: cart.items,
        total: cart.totalPrice,
        paymentInfo,
    }});
    await order.save();
    await Cart.findByIdAndDelete(cartId);
res.json({{ message: 'Order placed successfully', orderId: order._id }});
}} catch (err) {{
    console.error(err);
res.status(500).json({{ message: 'Server error' }});
    }}
}});
```
</content>
</rerank_entry>
<rerank_entry>
<id>
4
</id>
<content>
Code Location: user_model.js:1-10
const mongoose = require('mongoose');
const userSchema = new mongoose.Schema({{
email: {{
    type: String,
    required: true,
    unique: true
    }},
password: {{
    type: String,
    required: true
    }},
    name: String,
    address: String,
    phone: String,
isAdmin: {{
    type: Boolean,
    default: false  
    }}
}}, {{ timestamps: true }});
module.exports = mongoose.model('User', userSchema);
</content>
</rerank_entry>
<rerank_entry>
<id>
5
</id>
<content>
Code Location: index.js:10-25
```typescript
const express = require('express');
const mongoose = require('mongoose');
const session = require('express-session');
const MongoStore = require('connect-mongo')(session);
const app = express();
mongoose.connect(process.env.MONGO_URI, {{
    useNewUrlParser: true,
    useUnifiedTopology: true
}});
app.use(express.json());
app.use(session({{
    secret: process.env.SESSION_SECRET,
    resave: false,
    saveUninitialized: true,
store: new MongoStore({{ mongooseConnection: mongoose.connection }})
}}));
app.use('/auth', require('./routes/auth'));
app.use('/cart', require('./routes/cart'));  
app.use('/checkout', require('./routes/checkout'));
app.use('/orders', require('./routes/order'));
app.use('/products', require('./routes/product'));
app.use((err, req, res, next) => {{
    console.error(err);
res.status(500).json({{ message: 'Server error' }});
}});
const PORT = process.env.PORT || 5000;
app.listen(PORT, () => console.log(`Server started on port ${{PORT}}`));
```
</content>
</rerank_entry>
<rerank_entry>
<id>
6
</id>
<content>
Code Loction: payment.js:3-20
```typescript
const stripe = require('stripe')(process.env.STRIPE_SECRET_KEY);
router.post('/charge', async (req, res) => {{
const {{ amount, token }} = req.body;
try {{
const charge = await stripe.charges.create({{
        amount,
        currency: 'usd',
        source: token,
        description: 'Example charge'
    }});
res.json({{ message: 'Payment successful', charge }});
}} catch (err) {{
    console.error(err);  
res.status(500).json({{ message: 'Payment failed' }});
    }}
}});
```
</content>
</rerank_entry>
<rerank_entry>
<id>
7
</id>
<content>
Code Loction: product_model.js:1-12
```typescript
const mongoose = require('mongoose');
const productSchema = new mongoose.Schema({{
name: {{
    type: String,
    required: true
    }},
    description: String,
price: {{
    type: Number,
    required: true,
    min: 0
    }},
category: {{
    type: String,
    enum: ['electronics', 'clothing', 'home'],
    required: true  
    }},
stock: {{
    type: Number,
    default: 0,
    min: 0
    }}
}});
module.exports = mongoose.model('Product', productSchema);
```
</content>
</rerank_entry>
<rerank_entry>
<id>
8
</id>
<content>
Code Location: order_model.js:1-15
```typescript
const mongoose = require('mongoose');
const orderSchema = new mongoose.Schema({{
user: {{ 
    type: mongoose.Schema.Types.ObjectId,
    ref: 'User',
    required: true
    }},
items: [{{
product: {{
        type: mongoose.Schema.Types.ObjectId,
        ref: 'Product'
    }},
    quantity: Number,
    price: Number
    }}],
total: {{
    type: Number,
    required: true
    }},
paymentInfo: {{
    type: Object,
    required: true
    }},
status: {{
    type: String,
    enum: ['pending', 'processing', 'shipped', 'delivered'],
    default: 'pending'
    }}
}}, {{ timestamps: true }});
module.exports = mongoose.model('Order', orderSchema);
```
</content>
</rerank_entry>

<rerank_entry>
<id>
9
</id>
<content>
Code Location: cart.js:5-20
```typescript
router.post('/add', async (req, res) => {{
const {{ productId, quantity }} = req.body;
    
try {{
let cart = await Cart.findOne({{ user: req.user._id }});
if (cart) {{
        const itemIndex = cart.items.findIndex(item => item.product == productId);
if (itemIndex > -1) {{
        cart.items[itemIndex].quantity += quantity;
}} else {{
cart.items.push({{ product: productId, quantity, price: product.price }});
        }}
        cart = await cart.save();
}} else {{
cart = await Cart.create({{
        user: req.user._id,
items: [{{ product: productId, quantity, price: product.price }}]
        }});
    }}
    res.json(cart);
}} catch (err) {{
    console.error(err);
res.status(500).json({{ message: 'Server error' }});  
    }}
}});
```
</content>
</rerank_entry>
</rerank_list>

Your reply should be:

<thinking>
We want to get the relevant code for handling the checkout process since that has the error. The checkout and the payment along with how the order schema is handled seems relevant to the user query.
</thinking>

<code_to_probe_list>
<code_to_probe>
<id>
3
</id>
<reason_to_probe>
This code handles the checkout process. It receives the cart ID and payment info from the request body. It finds the cart, creates a new order with the cart items and payment info, saves the order, deletes the cart, and returns the order ID. This is likely where the issue is occurring.
</reason_to_probe>
</code_to_probe>
<code_to_probe>
<id>
6
</id>
<reason_to_probe>
This code processes the actual payment by creating a Stripe charge. The payment info comes from the checkout process. If the payment fails, that could explain the checkout error, so this is important to investigate.
</reason_to_probe>
</code_to_probe>
<code_to_probe>
<id>
8
</id>
<reason_to_probe>
This defines the schema and model for orders. An order contains references to the user and product items, the total price, payment info, and status. It's important for understanding the structure of an order, but unlikely to contain bugs.
</reason_to_probe>
</code_to_probe>
</code_to_probe_list>
</example>

Always remember that you have to reply in the following format:
<code_to_probe_list>
{list of snippets we want to probe}
</code_to_probe_list>
If there are no snippets which need to be probed then reply with an emply list of items for <code_to_probe_list>."#.to_owned()
    }

    fn system_message_for_probing(&self) -> String {
        let example_message = self.example_message_for_probing();
        let _ = format!(
            r#"You are an expert software engineer who knows how to find the code snippets which are relevant or interesting to understand for the user query.
- The code snippets will be provided to you in <code_snippet> section which will also have an id in the <id> section.
- First think step by step on how you want to go about selecting the code snippets which are relevant to the user query in max 50 words in a xml section called <thinking>. Do this first and then continue with helping with the user query.
- The code snippet which you select will be passed to another software engineer who is going to use it and deeply understand it to help answer the user query.
- The code snippet which you select might also have code symbols (variables, classes, function calls etc) inside it which we can click and follow to understand and gather more information, remember this when selecting the code snippets.
- You have to order the code snippets in the order of important, and only include the sections which will be part of the additional understanding or contain the answer to the user query, pug these code symbols in the <code_to_probe_list>
- If you want to deeply understand the section with id 0 then you must output in the following format:
<code_to_probe>
<id>
0
</id>
<reason_to_probe>
{{your reason for probing}}
</reason_to_probe>
</code_to_probe>

- If you want to edit more code sections follow the similar pattern as described above and as an example again:
<code_to_probe_list>
<code_to_probe>
<id>
{{id of the code snippet you are interested in}}
</id>
<reason_to_probe>
{{your reason for probing or understanding this section more deeply and the details on what you want to understand}}
</reason_to_probe>
</code_to_probe>
{{... more code sections here which you might want to select}}
</code_to_probe_list>

- The <id> section should ONLY contain an id from the listed code snippets.

{example_message}

This example is for reference. You must strictly follow the format show in the example when replying.
Please provide the list of symbols which you want to edit."#
        );
        format!(
            r#"You are a powerful code filtering engine. You have to order the code snippets in the order in which you want to ask them more questions, you will only get to ask these code snippets deeper questions by following various code symbols to their definitions or references.
- Probing a code snippet implies that you can follow the type of a symbol or function call or declaration if you think we should be following that symbol. 
- The code snippets which you want to probe more should be part of the <code_to_probe_list> section.
- The code snippets will be provided to you in <code_snippet> section which will also have an id in the <id> section.
- You have to order the code to probe snippets in the order of importance, and only include code sections which are part of the <code_to_probe_list>
- If you want to ask the section with id 0 then you must output in the following format:
<code_to_probe>
<id>
0
</id>
<reason_to_probe>
{{your reason for probing}}
</reason_to_probe>
</code_to_probe>

Here is the example contained in the <example> section.

{example_message}

These example is for reference. You must strictly follow the format shown in the example when replying.

Some more examples of outputs and cases you need to handle:
<example>
<scenario>
there are some code sections which are present in <code_to_probe_list>
</scenario>
<output>
</code_to_probe_list>
<code_to_probe>
<id>
0
</id>
<reason_to_probe>
{{your reason for probing this code section}}
</reason_to_probe>
</code_to_probe>
{{more code to probe list items...}}
</code_to_probe_list>

Notice how we include the elements for <code_to_probe_list>
</example>
<example>
<scenario>
there are no <code_to_probe_list> items
</scenario>
<output>
<code_to_probe_list>
</code_to_probe_list>
</output>
</example>

In this example we still include the <code_to_probe_list> section even if there are no code sections which we need to probe.

Please provide the order along with the reason in 2 lists, one for code snippets which you want to probe and the other for symbols we do not have to probe to answer the user query."#
        )
    }

    fn system_message_code_to_edit_symbol_level(&self) -> String {
        let example_message = self.example_message_for_filtering_code_edit_block();
        format!(r#"You are a powerful code filtering engine. You must order the code snippets in the order in you want to edit them, and only those code snippets which should be edited.
- The code snippets will be provided to you in <code_snippet> section which will also have an id in the <id> section.
- You should select a code section for editing if and only if you want to make changes to that section.
- You are also given a list of extra symbols in <extra_symbols> which will be provided to you while making the changes, use this to be more sure about your reason for selection.
- Adding new functionality is a very valid reason to select a sub-section for editing.
- Editing or deleting some code is also a very valid reason for selecting a code section for editing.
- First think step by step on how you want to go about selecting the code snippets which are relevant to the user query in max 50 words.
- If you want to edit the code section with id 0 then you must output in the following format:
<code_to_edit_list>
<code_to_edit>
<id>
0
</id>
<reason_to_edit>
{{your reason for editing}}
</reason_to_edit>
</code_to_edit>
</code_to_edit_list>

- If you want to edit more code sections follow the similar pattern as described above and as an example again:
<code_to_edit_list>
<code_to_edit>
<id>
{{id of the code snippet you are interested in}}
</id>
<reason_to_edit>
{{your reason for editing}}
</reason_to_edit>
</code_to_edit>
{{... more code sections here which you might want to select}}
</code_to_edit_list>

- The <id> section should ONLY contain an id from the listed code snippets.


Here is an example contained in the <example> section.

{example_message}

This example is for reference. You must strictly follow the format show in the example when replying.
Please provide the list of symbols which you want to edit."#).to_owned()
    }

    fn _system_message(&self) -> String {
        let example_message = self._example_message();
        format!(r#"You are a powerful code filtering engine. You must order the code snippets in the order in you want to edit them, and only those code snippets which should be edited.
- The code snippets will be provided to you in <code_snippet> section which will also have an id in the <id> section.
- You should select a code section for editing if and only if you want to make changes to that section.
- You are also given a list of extra symbols in <extra_symbols> which will be provided to you while making the changes, use this to be more sure about your reason for selection.
- Adding new functionality is a very valid reason to select a sub-section for editing.
- Editing or deleting some code is also a very valid reason for selecting a code section for editing.
- First think step by step on how you want to go about selecting the code snippets which are relevant to the user query in max 50 words.
- If you want to edit the code section with id 0 then you must output in the following format:
<code_to_edit_list>
<code_to_edit>
<id>
0
</id>
<reason_to_edit>
{{your reason for editing}}
</reason_to_edit>
</code_to_edit>
</code_to_edit_list>

- If you want to edit more code sections follow the similar pattern as described above and as an example again:
<code_to_edit_list>
<code_to_edit>
<id>
{{id of the code snippet you are interested in}}
</id>
<reason_to_edit>
{{your reason for editing}}
</reason_to_edit>
</code_to_edit>
{{... more code sections here which you might want to select}}
</code_to_edit_list>

- The <id> section should ONLY contain an id from the listed code snippets.


Here is an example contained in the <example> section.

{example_message}

This example is for reference. You must strictly follow the format show in the example when replying.
Please provide the list of symbols which you want to edit."#).to_owned()
    }

    fn unescape_xml(&self, s: String) -> String {
        s.replace("\"", "&quot;")
            .replace("'", "&apos;")
            .replace(">", "&gt;")
            .replace("<", "&lt;")
            .replace("&", "&amp;")
    }

    fn parse_response_section(&self, response: &str) -> String {
        let mut is_inside_reason = false;
        let mut new_lines = vec![];
        for line in response.lines() {
            if line == "<reason_to_edit>"
                || line == "<reason_to_not_edit>"
                || line == "<reason_to_probe>"
                || line == "<reason_to_not_probe>"
            {
                is_inside_reason = true;
                new_lines.push(line.to_owned());
                continue;
            } else if line == "</reason_to_edit>"
                || line == "</reason_to_not_edit>"
                || line == "</reason_to_probe>"
                || line == "</reason_to_not_probe>"
            {
                is_inside_reason = false;
                new_lines.push(line.to_owned());
                continue;
            }
            if is_inside_reason {
                new_lines.push(self.unescape_xml(line.to_owned()));
            } else {
                new_lines.push(line.to_owned());
            }
        }
        new_lines.join("\n")
    }

    fn format_snippet(&self, idx: usize, snippet: &Snippet) -> String {
        let code_location = snippet.file_path();
        let range = snippet.range();
        let start_line = range.start_line();
        let end_line = range.end_line();
        let content = snippet.content();
        let language = snippet.language();
        format!(
            r#"<rerank_entry>
<id>
{idx}
</id>
<content>
Code location: {code_location}:{start_line}-{end_line}
```{language}
{content}
```
</content>
</rerank_entry>"#
        )
        .to_owned()
    }

    fn parse_code_sections(
        &self,
        response: &str,
    ) -> Result<CodeToEditSymbolResponse, CodeToEditFilteringError> {
        // first we want to find the code_to_edit and code_to_not_edit sections
        let mut code_to_edit_list = response
            .lines()
            .into_iter()
            .skip_while(|line| !line.contains("<code_to_edit_list>"))
            .skip(1)
            .take_while(|line| !line.contains("</code_to_edit_list>"))
            .collect::<Vec<&str>>()
            .join("\n");
        code_to_edit_list = format!(
            "<code_to_edit_list>
{code_to_edit_list}
</code_to_edit_list>"
        );
        code_to_edit_list = self.parse_response_section(&code_to_edit_list);
        let code_to_edit_list = from_str::<CodeToEditList>(&code_to_edit_list)
            .map_err(|e| CodeToEditFilteringError::SerdeError(e))?;
        Ok(CodeToEditSymbolResponse::new(
            code_to_edit_list,
            CodeToNotEditList::new(),
        ))
    }

    fn parse_response_for_probing_list(
        &self,
        response: &str,
    ) -> Result<CodeToProbeSymbolResponse, CodeToEditFilteringError> {
        // first we want to find the code_to_edit and code_to_not_edit sections
        let mut code_to_probe_list = response
            .lines()
            .into_iter()
            .skip_while(|line| !line.contains("<code_to_probe_list>"))
            .skip(1)
            .take_while(|line| !line.contains("</code_to_probe_list>"))
            .collect::<Vec<&str>>()
            .join("\n");
        code_to_probe_list = format!(
            "<code_to_probe_list>
{code_to_probe_list}
</code_to_probe_list>"
        );
        code_to_probe_list = self.parse_response_section(&code_to_probe_list);
        let code_to_probe_list = from_str::<CodeToProbeList>(&code_to_probe_list)
            .map_err(|e| CodeToEditFilteringError::SerdeError(e))?;
        Ok(CodeToProbeSymbolResponse::new(code_to_probe_list))
    }

    fn parse_reponse_for_probing(
        &self,
        response: &str,
        snippets: &[Snippet],
    ) -> Result<CodeToProbeFilterResponse, CodeToEditFilteringError> {
        let response = self.parse_response_for_probing_list(response)?;
        let code_to_probe_list = response.code_to_probe_list();
        let snippet_mapping = snippets
            .into_iter()
            .enumerate()
            .collect::<HashMap<usize, &Snippet>>();
        let code_to_probe_list = code_to_probe_list
            .snippets()
            .into_iter()
            .filter_map(|code_to_edit| {
                let snippet = snippet_mapping.get(&code_to_edit.id());
                if let Some(snippet) = snippet {
                    Some(SnippetWithReason::new(
                        (*snippet).clone(),
                        code_to_edit.reason_to_probe().to_owned(),
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        Ok(CodeToProbeFilterResponse::new(code_to_probe_list, vec![]))
    }

    fn parse_response(
        &self,
        response: &str,
        snippets: &[Snippet],
    ) -> Result<CodeToEditFilterResponse, CodeToEditFilteringError> {
        let response = self.parse_code_sections(response)?;
        let code_to_edit_list = response.code_to_edit_list();
        let code_to_not_edit_list = response.code_to_not_edit_list();
        let snippet_mapping = snippets
            .into_iter()
            .enumerate()
            .collect::<HashMap<usize, &Snippet>>();
        let mut code_to_edit_ids: HashSet<usize> = Default::default();
        let code_to_edit_list = code_to_edit_list
            .snippets()
            .into_iter()
            .filter_map(|code_to_edit| {
                let snippet = snippet_mapping.get(&code_to_edit.id());
                if let Some(snippet) = snippet {
                    code_to_edit_ids.insert(code_to_edit.id());
                    Some(SnippetWithReason::new(
                        (*snippet).clone(),
                        code_to_edit.reason_to_edit().to_owned(),
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let code_to_not_edit = code_to_not_edit_list
            .snippets()
            .into_iter()
            .filter_map(|code_to_not_edit| {
                let snippet = snippet_mapping.get(&code_to_not_edit.id());
                // If we have this in the list of code snippets to edit, then we
                // do not need to contain it in the list for code_to_not_edit
                // ideally the LLM does not make mistakes like this, but it does
                if code_to_edit_ids.contains(&code_to_not_edit.id()) {
                    return None;
                }
                if let Some(snippet) = snippet {
                    Some(SnippetWithReason::new(
                        (*snippet).clone(),
                        code_to_not_edit.reason_to_not_edit().to_owned(),
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        Ok(CodeToEditFilterResponse::new(
            code_to_edit_list,
            code_to_not_edit,
        ))
    }
}

#[async_trait]
impl CodeToEditFilterFormatter for AnthropicCodeToEditFormatter {
    async fn filter_code_snippets_inside_symbol(
        &self,
        request: CodeToEditSymbolRequest,
    ) -> Result<CodeToEditSymbolResponse, CodeToEditFilteringError> {
        // Here the only difference is that we are asking
        // for the sections to edit in a single symbol
        let root_request_id = request.root_request_id().to_owned();
        let query = request.query().to_owned();
        let request_llm = request.llm().clone();
        let request_provider = request.provider().clone();
        let request_api_key = request.api_key().clone();
        let extra_symbols = request
            .extra_symbols()
            .map(|extra_symbols_string| extra_symbols_string.to_owned())
            .unwrap_or("".to_owned());
        let xml_string = request.xml_string();
        let user_query = format!(
            r#"<user_query>
{query}
</user_query>

<extra_symbols>
{extra_symbols}
</extra_symbols>

{xml_string}"#
        );
        let system_message = self.system_message_code_to_edit_symbol_level();
        let messages = vec![
            LLMClientMessage::system(system_message),
            LLMClientMessage::user(user_query),
        ];
        let llm_request = LLMClientCompletionRequest::new(request_llm.clone(), messages, 0.1, None);
        let mut retries = 0;
        loop {
            if retries >= 4 {
                return Err(CodeToEditFilteringError::RetriesExhausted);
            }
            let (llm, api_key, provider) = if retries % 2 == 0 {
                (
                    request_llm.clone(),
                    request_api_key.clone(),
                    request_provider.clone(),
                )
            } else {
                (
                    self.fail_over_llm.llm().clone(),
                    self.fail_over_llm.api_key().clone(),
                    self.fail_over_llm.provider().clone(),
                )
            };
            let cloned_llm_request = llm_request.clone().set_llm(llm);
            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let response = self
                .llm_broker
                .stream_completion(
                    api_key,
                    cloned_llm_request,
                    provider,
                    vec![
                        (
                            "event_type".to_owned(),
                            "code_snippet_to_edit_for_symbol".to_owned(),
                        ),
                        ("root_id".to_owned(), root_request_id.to_owned()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await
                .map_err(|e| CodeToEditFilteringError::LLMClientError(e));
            match response {
                Ok(response) => {
                    if let Ok(parsed_response) =
                        self.parse_code_sections(response.answer_up_until_now())
                    {
                        return Ok(parsed_response);
                    } else {
                        retries = retries + 1;
                        continue;
                    }
                }
                Err(_e) => {
                    retries = retries + 1;
                    continue;
                }
            }
        }
    }

    async fn filter_code_snippets(
        &self,
        request: CodeToEditFilterRequest,
    ) -> Result<CodeToEditFilterResponse, CodeToEditFilteringError> {
        // okay now we have the request, send it to the moon and figure out what to
        // do next with it
        let root_request_id = request.root_request_id().to_owned();
        let query = request.query();
        let input_list_for_entries = request
            .get_snippets()
            .into_iter()
            .enumerate()
            .map(|(idx, input)| self.format_snippet(idx, input))
            .collect::<Vec<_>>();
        let input_formatted = input_list_for_entries.join("\n");
        let user_query = format!(
            r#"<user_query>
{query}
</user_query>

<rerank_list>
{input_formatted}
</rerank_list>"#
        );
        let system_message = self.system_message_code_to_edit_symbol_level();
        let messages = vec![
            LLMClientMessage::system(system_message),
            LLMClientMessage::user(user_query),
        ];
        let llm_request =
            LLMClientCompletionRequest::new(request.llm().clone(), messages, 0.1, None);
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let response = self
            .llm_broker
            .stream_completion(
                request.api_key().clone(),
                llm_request,
                request.provider().clone(),
                vec![
                    ("event_type".to_owned(), "code_snippets_to_edit".to_owned()),
                    ("root_id".to_owned(), root_request_id.to_owned()),
                ]
                .into_iter()
                .collect(),
                sender,
            )
            .await
            .map_err(|e| CodeToEditFilteringError::LLMClientError(e))?;

        // Now to parse that output and reply back to the asking person
        // TODO(skcd):
        // we need to figure out how to parse the output back, it should be easy
        // as its well formatted xml
        // and then we need to change the return types here from raw snippets
        // to snippets with reason to edit and not to edit
        self.parse_response(response.answer_up_until_now(), request.get_snippets())
    }

    async fn filter_code_snippet_inside_symbol_for_probing(
        &self,
        request: CodeToEditFilterRequest,
    ) -> Result<CodeToProbeFilterResponse, CodeToEditFilteringError> {
        let root_request_id = request.root_request_id().to_owned();
        let query = request.query().to_owned();
        let input_list_for_entries = request
            .get_snippets()
            .into_iter()
            .enumerate()
            .map(|(idx, input)| self.format_snippet(idx, input))
            .collect::<Vec<_>>();
        let input_formatted = input_list_for_entries.join("\n");
        let user_query = format!(
            r#"<user_query>
{query}
</user_query>

<rerank_list>
{input_formatted}
</rerank_list>

Remember that your reply should be strictly in the following format:
<code_to_probe_list>
{{list of snippets we want to probe in the format specified}}
</code_to_probe_list>"#
        );
        let system_message = self.system_message_for_probing();
        let messages = vec![
            LLMClientMessage::system(system_message),
            LLMClientMessage::user(user_query),
        ];
        let llm_request =
            LLMClientCompletionRequest::new(request.llm().clone(), messages, 0.1, None);
        let mut retries = 0;
        let root_request_id_ref = &root_request_id;
        loop {
            if retries > 3 {
                return Err(CodeToEditFilteringError::RetriesExhausted);
            }
            if retries != 0 {
                jitter_sleep(5.0, retries as f64).await;
            }
            // alternate between the fail-over llm and the normal one
            let mut provider = request.provider().clone();
            let mut api_key = request.api_key().clone();
            let mut llm_request_cloned = llm_request.clone();
            if retries % 2 == 0 {
                llm_request_cloned = llm_request_cloned.set_llm(request.llm().clone());
            } else {
                llm_request_cloned = llm_request_cloned.set_llm(self.fail_over_llm.llm().clone());
                provider = self.fail_over_llm.provider().clone();
                api_key = self.fail_over_llm.api_key().clone();
            }
            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let response = self
                .llm_broker
                .stream_completion(
                    api_key.clone(),
                    llm_request_cloned.clone(),
                    provider.clone(),
                    vec![
                        (
                            "event_type".to_owned(),
                            "filter_code_snippet_for_probing".to_owned(),
                        ),
                        ("root_id".to_owned(), root_request_id_ref.to_string()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await
                .map_err(|e| CodeToEditFilteringError::LLMClientError(e))?;
            if response.answer_up_until_now().is_empty() {
                retries = retries + 1;
                continue;
            }
            let result = self
                .parse_reponse_for_probing(response.answer_up_until_now(), request.get_snippets());
            match result {
                Ok(_) => return result,
                Err(_) => {
                    retries = retries + 1;
                    continue;
                }
            };
        }
    }

    async fn filter_code_snippets_probing_sub_symbols(
        &self,
        request: CodeToProbeSubSymbolRequest,
    ) -> Result<CodeToProbeSubSymbolList, CodeToEditFilteringError> {
        println!("code_to_edit_filter_formatter::filter_code_snippets_probing_sub_symbols");
        let root_request_id = request.root_request_id().to_owned();
        let query = request.query().to_owned();
        let xml_string = request.xml_symbol();
        let user_query = format!(
            r#"<user_query>
{query}
</user_query>

{xml_string}"#
        );
        let system_message = self.system_message_for_probing();
        let messages = vec![
            LLMClientMessage::system(system_message),
            LLMClientMessage::user(user_query),
        ];
        let llm_request =
            LLMClientCompletionRequest::new(request.llm().clone(), messages, 0.1, None);
        let mut retries = 0;
        let root_request_id_ref = &root_request_id;
        loop {
            if retries > 3 {
                return Err(CodeToEditFilteringError::RetriesExhausted);
            }
            if retries != 0 {
                jitter_sleep(5.0, retries as f64).await;
            }
            // alternate between the fail-over llm and the normal one
            let mut provider = request.provider().clone();
            let mut api_key = request.api_key().clone();
            let mut llm_request_cloned = llm_request.clone();
            if retries % 2 == 0 {
                llm_request_cloned = llm_request_cloned.set_llm(request.llm().clone());
            } else {
                llm_request_cloned = llm_request_cloned.set_llm(self.fail_over_llm.llm().clone());
                provider = self.fail_over_llm.provider().clone();
                api_key = self.fail_over_llm.api_key().clone();
            }
            let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
            let response = self
                .llm_broker
                .stream_completion(
                    api_key.clone(),
                    llm_request_cloned.clone(),
                    provider.clone(),
                    vec![
                        (
                            "event_type".to_owned(),
                            "filter_code_snippet_sub_sybmol_probing".to_owned(),
                        ),
                        ("root_id".to_owned(), root_request_id_ref.to_owned()),
                    ]
                    .into_iter()
                    .collect(),
                    sender,
                )
                .await
                .map_err(|e| CodeToEditFilteringError::LLMClientError(e))?;
            if response.answer_up_until_now().is_empty() {
                retries = retries + 1;
                continue;
            }
            let result = CodeToProbeSubSymbolList::from_string(response.answer_up_until_now());
            match result {
                Ok(_) => return result,
                Err(_) => {
                    retries = retries + 1;
                    continue;
                }
            };
        }
    }
}
