//! We can use a listwise iterator over here to get the useful utilities which might
//! be required for the agent to perform an action, this can be run in the background
//! The goal is to keep reranking snippets of code
//!
//! Once we are able to rerank freely, we will be able to do a lot more

use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc};

use llm_client::{
    broker::LLMBroker,
    clients::types::{LLMClientCompletionRequest, LLMClientMessage},
};

use crate::agentic::tool::rerank::base::{
    ReRank, ReRankEntries, ReRankEntry, ReRankError, ReRankRequestMetadata,
};

pub struct AnthropicReRank {
    llm_client: Arc<LLMBroker>,
}

impl AnthropicReRank {
    pub fn new(llm_client: Arc<LLMBroker>) -> Self {
        Self { llm_client }
    }

    fn format_input(&self, input: &ReRankEntries) -> String {
        let id = input.id();
        let entry = input.entry();
        match entry {
            ReRankEntry::CodeSnippet(code_snippet) => {
                let range = code_snippet.range();
                let start_line = range.start_line();
                let end_line = range.end_line();
                let fs_file_path = code_snippet.fs_file_path();
                let content = code_snippet.content();
                let language = code_snippet.language();
                format!(
                    r#"<rerank_entry>
<id>
{id}
</id>
<content>
Code Location: {fs_file_path}:{start_line}-{end_line}
```{language}
{content}
```
</content>
</rerank_entry>"#
                )
            }
            ReRankEntry::Document(document) => {
                let path = document.document_path();
                let content = document.content();
                format!(
                    r#"<rerank_entry>
<id>
{id}
</id>
<content>
Path: {path}
{content}
</content>
</rerank_entry>"#
                )
            }
            ReRankEntry::WebExtract(web_extract) => {
                let url = web_extract.url();
                let content = web_extract.content();
                format!(
                    r#"<rerank_entry>
<id>
{id}
</id>
<content>
WebPage: {url}
{content}
</content>
</rerank_entry>"#
                )
            }
        }
    }

    fn example_message(&self) -> String {
        format!(
            r#"<user_query>
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

<explanations>
0
This code handles user registration and login. It's used to authenticate the user before checkout can occur. But since the error happens after entering payment info, authentication is likely not the problem.

1
This defines the schema and model for shopping carts. A cart contains references to the user and product items. It also has a virtual property to calculate the total price. It's used in the checkout process but probably not the source of the bug.

2
This code allows fetching the logged-in user's orders. It's used after the checkout process to display order history. It doesn't come into play until after checkout is complete.

3
This code handles the checkout process. It receives the cart ID and payment info from the request body. It finds the cart, creates a new order with the cart items and payment info, saves the order, deletes the cart, and returns the order ID. This is likely where the issue is occurring.

4
This defines the schema and model for user accounts. A user has an email, password, name, address, phone number, and admin status. The user ID is referenced by the cart and order, but the user model itself is not used in the checkout.

6
This is the main Express server file. It sets up MongoDB, middleware, routes, and error handling. While it's crucial for the app as a whole, it doesn't contain any checkout-specific logic.

7
This code processes the actual payment by creating a Stripe charge. The payment info comes from the checkout process. If the payment fails, that could explain the checkout error, so this is important to investigate.

8
This defines the schema and model for products. A product has a name, description, price, category, and stock quantity. It's referenced by the cart and order models but is not directly used in the checkout process.

9
This defines the schema and model for orders. An order contains references to the user and product items, the total price, payment info, and status. It's important for understanding the structure of an order, but unlikely to contain bugs.

10
This code handles adding items to the cart. It's used before the checkout process begins. While it's important for the overall shopping flow, it's unlikely to be directly related to a checkout bug.  
</explanations>

<ranking>
3
7 
9
1
6
0
10
2
4
8
</ranking>
</example>"#
        )
    }

    fn system_message(&self) -> String {
        let example_message = self.example_message();
        format!(
            r#"You are a powerful code search engine. You must order the list of code snippets from the most relevant to the least relevant to the user's query. You must order ALL TEN snippets.
First, for each code snippet, provide a brief explanation of what the code does and how it relates to the user's query.

Then, rank the snippets based on relevance. The most relevant files are the ones we need to edit to resolve the user's issue. The next most relevant snippets are dependencies - code that is crucial to read and understand while editing the other files to correctly resolve the user's issue.

Note: For each code snippet, provide an explanation of what the code does and how it fits into the overall system, even if it's not directly relevant to the user's query. The ranking should be based on relevance to the query, but all snippets should be explained.

The response format is:
<explanations>
<file_path:start_line-end_line>
Explanation of what the code does, regardless of its relevance to the user's query. Provide context on how it fits into the overall system.
file_path:start_line-end_line
Explanation of what the code does, regardless of its relevance to the user's query. Provide context on how it fits into the overall system.
file_path:start_line-end_line
Explanation of what the code does, regardless of its relevance to the user's query. Provide context on how it fits into the overall system.
file_path:start_line-end_line
Explanation of what the code does, regardless of its relevance to the user's query. Provide context on how it fits into the overall system.
file_path:start_line-end_line
Explanation of what the code does, regardless of its relevance to the user's query. Provide context on how it fits into the overall system.
file_path:start_line-end_line
Explanation of what the code does, regardless of its relevance to the user's query. Provide context on how it fits into the overall system.
file_path:start_line-end_line
Explanation of what the code does, regardless of its relevance to the user's query. Provide context on how it fits into the overall system.
file_path:start_line-end_line
Explanation of what the code does, regardless of its relevance to the user's query. Provide context on how it fits into the overall system.
file_path:start_line-end_line
Explanation of what the code does, regardless of its relevance to the user's query. Provide context on how it fits into the overall system.
file_path:start_line-end_line
Explanation of what the code does, regardless of its relevance to the user's query. Provide context on how it fits into the overall system.
</explanations>

<ranking>
first_most_relevant_snippet
second_most_relevant_snippet
third_most_relevant_snippet
fourth_most_relevant_snippet
fifth_most_relevant_snippet
sixth_most_relevant_snippet
seventh_most_relevant_snippet
eighth_most_relevant_snippet
ninth_most_relevant_snippet
tenth_most_relevant_snippet
</ranking>

Here is an example:

{example_message}

This example is for reference. Please provide explanations and rankings for the code snippets based on the user's query."#
        )
    }

    pub fn parse_ids(&self, output: &str) -> Vec<usize> {
        // the output has the following section
        // <ranking>
        // first_most_relevant_snippet
        // second_most_relevant_snippet
        // ...
        // </ranking>
        // so we find the <ranking> and then split into lines and grab that section
        output
            .lines()
            .skip_while(|predicate| predicate.trim() != "<ranking>")
            .into_iter()
            .skip(1)
            .take_while(|predicate| predicate.trim() != "</ranking>")
            .map(|line| line.trim().parse::<usize>().unwrap())
            .collect()
    }
}

#[async_trait]
impl ReRank for AnthropicReRank {
    async fn rerank(
        &self,
        input: Vec<ReRankEntries>,
        metadata: ReRankRequestMetadata,
    ) -> Result<Vec<ReRankEntries>, ReRankError> {
        let query = metadata.query();
        let input_list_for_entries = input
            .iter()
            .map(|input| self.format_input(input))
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
        let system_message = self.system_message();
        let messages = vec![
            LLMClientMessage::system(system_message),
            LLMClientMessage::user(user_query),
        ];
        let request =
            LLMClientCompletionRequest::new(metadata.model().clone(), messages, 0.2, None);
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        let api_key = metadata.provider_keys().clone();
        let provider = metadata.provider().clone();
        let response = self
            .llm_client
            .stream_completion(
                api_key,
                request,
                provider,
                vec![("event_type".to_owned(), "rerank".to_owned())]
                    .into_iter()
                    .collect(),
                sender,
            )
            .await
            .map_err(|e| ReRankError::LlmClientError(e))?;
        // now we parse out the ranked list from here
        let ids = self.parse_ids(response.answer_up_until_now());
        // now we re-order the input based on the ids
        let mut reordered_input = vec![];
        let mut mapped_input = input
            .into_iter()
            .enumerate()
            .collect::<HashMap<usize, ReRankEntries>>();
        for id in ids {
            if let Some(entry_at_reranked_position) = mapped_input.remove(&id) {
                reordered_input.push(entry_at_reranked_position);
            }
        }
        Ok(reordered_input)
    }
}
