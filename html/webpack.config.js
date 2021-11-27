"use strict";

const HtmlWebpackPlugin = require("html-webpack-plugin");
const MiniCssExtractPlugin = require("mini-css-extract-plugin");
const path = require("path");

const production = process.env.NODE_ENV === "production";

const pages = ["home", "dashboard"];

module.exports = {
	entry: Object.fromEntries(pages.map(page => [page, `./src/${page}.tsx`])),
	module: {
		rules: [
			{
				test: /\.tsx?$/,
				loader: "babel-loader",
			},
			{
				test: /\.s?css$/,
				use: [
					production ? MiniCssExtractPlugin.loader : "style-loader",
					"css-loader",
					"sass-loader",
				],
			},
		],
	},
	output: {
		filename: "assets/[name].[contenthash].js",
		path: path.resolve(__dirname, "dist"),
	},
	plugins: [
		new MiniCssExtractPlugin({ filename: "assets/[name].[contenthash].css" }),
		...pages.map(page => new HtmlWebpackPlugin({
			filename: `${page}.html`,
			template: `src/${page}.ejs`,
			chunks: [page],
			inject: "body",
		})),
	],
	optimization: {
		mangleExports: "size",
		moduleIds: "size",
	},
	watchOptions: {
		ignored: /node_modules|dist/,
	},
	resolve: {
		mainFiles: ["index"],
		extensions: [".js", ".ts", ".tsx"],
	},
	devtool: production ? false : "eval-cheap-module-source-map",
	mode: process.env.NODE_ENV,
};
